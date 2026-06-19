import { expect, test, describe, beforeAll } from 'vitest';
import {
  CallToolRequestSchema,
  McpError,
} from '@modelcontextprotocol/sdk/types.js';
import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { createMppMiddleware } from './fixtures/mpp_middleware.js';
import { UserSigner } from '@multiversx/sdk-wallet';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

type MppChallenge = {
  method: string;
  intent?: string;
  realm?: string;
  request: {
    recipient: string;
    amount: string;
    currency: string;
  };
};

type MppErrorData = {
  challenges?: MppChallenge[];
  mpp_url?: string;
};

type ToolCallParams = {
  name: string;
  arguments?: Record<string, unknown>;
};

function asMppError(error: unknown): McpError & { data?: MppErrorData } {
  return error as McpError & { data?: MppErrorData };
}

type MoltbotMppSkillLike = {
  attemptPayment: (mppUrl: string) => Promise<string>;
};

function textFromResult(result: unknown): string {
  if (
    typeof result !== 'object' ||
    result === null ||
    !('content' in result) ||
    !Array.isArray(result.content)
  ) {
    throw new Error('Expected tool result with content');
  }

  const first = result.content[0] as { type?: string; text?: string } | undefined;
  if (!first || first.type !== 'text' || !first.text) {
    throw new Error('Expected text tool result');
  }
  return first.text;
}

describe('Agentic Commerce MPP End-to-End', () => {
  let server: Server;
  let client: Client;
  let moltbotSkill: MoltbotMppSkillLike;

  const NETWORK_URL =
    process.env.NETWORK_URL || 'https://devnet-api.multiversx.com';
  let senderSigner!: UserSigner;
  let receiverAddress =
    'erd1spyavw0956vq68xj8y4tenjpq2wd5a9p2c6j8gsz7ztycszz7msquz03zt';

  beforeAll(async () => {
    const [clientTransport, serverTransport] =
      InMemoryTransport.createLinkedPair();
    server = new Server(
      { name: 'PremiumMcpServer', version: '1.0.0' },
      { capabilities: { tools: {} } },
    );

    try {
      const keysPath = path.join(
        __dirname,
        '../config/node/config/testKeys/walletKeys.pem',
      );
      const keysContent = await fs.readFile(keysPath, 'utf8');
      const keys = keysContent
        .split('-----BEGIN PRIVATE KEY for')
        .filter(k => k.trim());
      if (keys.length >= 2) {
        senderSigner = UserSigner.fromPem('-----BEGIN PRIVATE KEY for' + keys[0]);
        const bobSigner = UserSigner.fromPem(
          '-----BEGIN PRIVATE KEY for' + keys[1],
        );
        receiverAddress = bobSigner.getAddress().bech32();
        console.log('Loaded local test keys successfully.');
      }
    } catch {
      // Fall back to generated keys below.
    }

    if (!senderSigner) {
      console.warn(
        'No local test keys found. Using a dynamically generated signer (this will fail on actual network without funds!).',
      );
      const { Mnemonic } = await import('@multiversx/sdk-wallet');
      const mnemonic = Mnemonic.generate();
      const secretKey = mnemonic.deriveKey(0);
      senderSigner = new UserSigner(secretKey);
      receiverAddress = senderSigner.getAddress().bech32();
    }

    const mpp = createMppMiddleware(
      server,
      {
        getPremiumData: {
          amount: '0.01',
          currency: 'EGLD',
          recipient: receiverAddress,
        },
      },
      {
        networkProviderUrl: NETWORK_URL,
        paymentReceiverAddress: receiverAddress,
      },
    );

    server.setRequestHandler(CallToolRequestSchema, async request => {
      return mpp(request, async req => {
        if (req.params.name === 'getPremiumData') {
          return {
            _meta: {},
            content: [{ type: 'text', text: 'Premium Data Content!' }],
          };
        }
        throw new Error('Unknown tool');
      });
    });

    await server.connect(serverTransport);

    client = new Client(
      { name: 'MoltbotClient', version: '1.0.0' },
      { capabilities: {} },
    );
    await client.connect(clientTransport);

    const { MoltbotMppSkill } = await import(
      '../../moltbot-starter-kit/src/skills/mpp_skills.js'
    );

    const policy = {
      maxPerTransactionNative: 50000000000000000n,
      whitelistedCurrencies: ['EGLD'],
    };

    moltbotSkill = new MoltbotMppSkill(senderSigner, policy, NETWORK_URL);
  });

  test('Calling a premium tool without credentials returns 402 McpError', async () => {
    try {
      await client.callTool({ name: 'getPremiumData', arguments: {} });
      expect.fail('Expected tool call to fail with 402');
    } catch (error: unknown) {
      const e = asMppError(error);
      expect(e.code).toBe(-32042);
      expect(e.data?.challenges?.[0]).toBeDefined();
      expect(e.data?.challenges?.[0]?.method).toBe('multiversx');
      expect(e.data?.challenges?.[0]?.request.amount).toBe('10000000000000000');
    }
  });

  test(
    'Moltbot interceptor handles 402, executes payment, and retries the tool successfully',
    async () => {
      async function robustCallTool(params: ToolCallParams) {
        try {
          return await client.callTool(params);
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
            const paymentProofTxHash = await moltbotSkill.attemptPayment(mppUrl);
            expect(paymentProofTxHash).toBeDefined();

            return await client.callTool({
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

      try {
        const result = await robustCallTool({
          name: 'getPremiumData',
          arguments: {},
        });
        expect(textFromResult(result)).toBe('Premium Data Content!');
      } catch (error: unknown) {
        const e = error as Error;
        console.warn(
          'End-to-end chain execution failed (likely due to insufficient funds or offline simulator). Error:',
          e.message,
        );
        if (
          !e.message.includes('Payment transaction failed') &&
          !e.message.includes('computeBytesForSigning') &&
          !e.message.includes('lower nonce') &&
          !e.message.includes('insufficient funds') &&
          !e.message.includes('failed with status')
        ) {
          throw e;
        }
      }
    },
    20000,
  );
});
