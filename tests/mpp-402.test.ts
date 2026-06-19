import { expect, test, beforeAll } from 'vitest';
import { asMppError, createMppTestContext, type MppTestContext } from './fixtures/mpp_test_setup.js';

let ctx: MppTestContext;

beforeAll(async () => {
  ctx = await createMppTestContext();
});

test('Calling a premium tool without credentials returns 402 McpError', async () => {
  try {
    await ctx.client.callTool({ name: 'getPremiumData', arguments: {} });
    expect.fail('Expected tool call to fail with 402');
  } catch (error: unknown) {
    const e = asMppError(error);
    expect(e.code).toBe(-32042);
    expect(e.data?.challenges?.[0]).toBeDefined();
    expect(e.data?.challenges?.[0]?.method).toBe('multiversx');
    expect(e.data?.challenges?.[0]?.request.amount).toBe('10000000000000000');
    expect(e.data?.challenges?.[0]?.request.recipient).toBe(ctx.receiverAddress);
  }
});
