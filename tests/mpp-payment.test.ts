import { describe, expect, test, beforeAll } from 'vitest';
import {
  asMppError,
  createMppTestContext,
  textFromResult,
  type MppTestContext,
} from './fixtures/mpp_test_setup.js';

const RUN_CHAIN_TESTS = process.env.RUN_CHAIN_TESTS === '1';

type ToolCallParams = {
  name: string;
  arguments?: Record<string, unknown>;
};

type MoltbotMppSkillLike = {
  attemptPayment: (mppUrl: string) => Promise<string>;
};

describe.runIf(RUN_CHAIN_TESTS)('MPP chain payment', () => {
  let ctx: MppTestContext;
  let moltbotSkill: MoltbotMppSkillLike | undefined;

  beforeAll(async () => {
    ctx = await createMppTestContext();
    if (!ctx.hasFundedKeys) {
      return;
    }

    const { MoltbotMppSkill } = await import(
      '../../moltbot-starter-kit/src/skills/mpp_skills.js'
    );

    const policy = {
      maxPerTransactionNative: 50000000000000000n,
      whitelistedCurrencies: ['EGLD'],
    };

    moltbotSkill = new MoltbotMppSkill(ctx.senderSigner, policy, ctx.networkUrl);
  });

  test('requires funded local test keys', () => {
    expect(ctx.hasFundedKeys).toBe(true);
  });

  test(
    'Moltbot interceptor handles 402, executes payment, and retries the tool successfully',
    async () => {
      if (!ctx.hasFundedKeys || !moltbotSkill) {
        throw new Error('Chain test setup requires funded local test keys');
      }

      async function robustCallTool(params: ToolCallParams) {
        try {
          return await ctx.client.callTool(params);
        } catch (error: unknown) {
          const e = asMppError(error);
          const code = e.code;
          let mppUrl: string | undefined;

          if (code === -32042 && e.data?.challenges?.[0]) {
            const c = e.data.challenges[0];
            if (c.method === 'multiversx') {
              mppUrl = `mpp://${c.realm || 'localhost'}/${c.method}/${c.intent}?recipient=${c.request.recipient}&amount=${c.request.amount}&currency=${c.request.currency}`;
            }
          } else if (code === 402 && e.data?.mpp_url) {
            mppUrl = e.data.mpp_url;
          }

          if (mppUrl) {
            const paymentProofTxHash = await moltbotSkill!.attemptPayment(mppUrl);
            expect(paymentProofTxHash).toBeDefined();

            return await ctx.client.callTool({
              name: params.name,
              arguments: {
                ...(params.arguments ?? {}),
                _mpp_payment_proof: paymentProofTxHash,
              },
            });
          }
          throw error;
        }
      }

      const result = await robustCallTool({
        name: 'getPremiumData',
        arguments: {},
      });
      expect(textFromResult(result)).toBe('Premium Data Content!');
    },
    20_000,
  );
});
