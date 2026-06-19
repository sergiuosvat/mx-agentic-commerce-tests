import { McpError, type CallToolRequest } from '@modelcontextprotocol/sdk/types.js';
import type { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { ApiNetworkProvider } from '@multiversx/sdk-network-providers';

export type ToolPricing = Record<
  string,
  { amount: string; currency: string; recipient: string }
>;

export type MppMiddlewareConfig = {
  networkProviderUrl: string;
  paymentReceiverAddress: string;
};

type ToolCallResult = {
  _meta: Record<string, unknown>;
  content: Array<{ type: 'text'; text: string }>;
};

type CallToolHandler = (request: CallToolRequest) => Promise<ToolCallResult>;

const PAYMENT_REQUIRED_CODE = -32042;

function egldToWei(amount: string): string {
  const [whole = '0', fraction = ''] = amount.split('.');
  const paddedFraction = (fraction + '0'.repeat(18)).slice(0, 18);
  return (BigInt(whole) * 10n ** 18n + BigInt(paddedFraction || '0')).toString();
}

function receiverBech32(receiver: unknown): string {
  if (typeof receiver === 'string') {
    return receiver;
  }
  const candidate = receiver as { bech32?: () => string; toString?: () => string };
  if (typeof candidate.bech32 === 'function') {
    return candidate.bech32();
  }
  return candidate.toString?.() ?? String(receiver);
}

async function verifyPayment(
  txHash: string,
  expected: { amount: string; currency: string; recipient: string },
  networkProviderUrl: string,
): Promise<void> {
  const provider = new ApiNetworkProvider(networkProviderUrl);
  const tx = await provider.getTransaction(txHash);

  if (!tx.status.isSuccessful()) {
    throw new Error('Payment transaction failed on chain');
  }

  const receiver = receiverBech32(tx.receiver);
  if (receiver !== expected.recipient) {
    throw new Error(`Payment sent to wrong recipient: ${receiver}`);
  }

  if (expected.currency === 'EGLD') {
    const expectedWei = egldToWei(expected.amount);
    if (tx.value.toString() !== expectedWei) {
      throw new Error(
        `Payment amount mismatch: expected ${expectedWei}, got ${tx.value.toString()}`,
      );
    }
  }
}

export function createMppMiddleware(
  _server: Server,
  pricing: ToolPricing,
  config: MppMiddlewareConfig,
) {
  return async (
    request: CallToolRequest,
    next: CallToolHandler,
  ): Promise<ToolCallResult> => {
    const price = pricing[request.params.name];
    if (!price) {
      return next(request);
    }

    const args = (request.params.arguments ?? {}) as Record<string, unknown>;
    const proof = args._mpp_payment_proof;

    if (typeof proof !== 'string' || proof.length === 0) {
      const amountWei = egldToWei(price.amount);
      throw new McpError(PAYMENT_REQUIRED_CODE, 'Payment Required', {
        httpStatus: 402,
        challenges: [
          {
            method: 'multiversx',
            intent: 'charge',
            realm: 'localhost',
            request: {
              amount: amountWei,
              currency: price.currency,
              recipient: price.recipient || config.paymentReceiverAddress,
            },
          },
        ],
      });
    }

    await verifyPayment(proof, price, config.networkProviderUrl);

    const { _mpp_payment_proof: _proof, ...rest } = args;
    return next({
      ...request,
      params: {
        ...request.params,
        arguments: rest,
      },
    });
  };
}
