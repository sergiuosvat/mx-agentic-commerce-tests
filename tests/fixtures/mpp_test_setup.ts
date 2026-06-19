import { Server } from '@modelcontextprotocol/sdk/server/index.js';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';
import { createMppMiddleware } from './mpp_middleware.js';
import { UserSigner } from '@multiversx/sdk-wallet';
import { promises as fs } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export type MppChallenge = {
  method: string;
  intent?: string;
  realm?: string;
  request: {
    recipient: string;
    amount: string;
    currency: string;
  };
};

export type MppErrorData = {
  challenges?: MppChallenge[];
  mpp_url?: string;
};

export type MppTestContext = {
  server: Server;
  client: Client;
  receiverAddress: string;
  networkUrl: string;
  senderSigner: UserSigner;
  hasFundedKeys: boolean;
};

export function asMppError(error: unknown): import('@modelcontextprotocol/sdk/types.js').McpError & {
  data?: MppErrorData;
} {
  return error as import('@modelcontextprotocol/sdk/types.js').McpError & {
    data?: MppErrorData;
  };
}

export function textFromResult(result: unknown): string {
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

export async function createMppTestContext(): Promise<MppTestContext> {
  const networkUrl =
    process.env.NETWORK_URL || 'https://devnet-api.multiversx.com';

  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  const server = new Server(
    { name: 'PremiumMcpServer', version: '1.0.0' },
    { capabilities: { tools: {} } },
  );

  let senderSigner!: UserSigner;
  let receiverAddress =
    'erd1spyavw0956vq68xj8y4tenjpq2wd5a9p2c6j8gsz7ztycszz7msquz03zt';
  let hasFundedKeys = false;

  try {
    const keysPath = path.join(
      __dirname,
      '../../config/node/config/testKeys/walletKeys.pem',
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
      hasFundedKeys = true;
    }
  } catch {
    // Fall back to generated keys for unit-only runs.
  }

  if (!senderSigner) {
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
      networkProviderUrl: networkUrl,
      paymentReceiverAddress: receiverAddress,
    },
  );

  const { CallToolRequestSchema } = await import('@modelcontextprotocol/sdk/types.js');
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

  const client = new Client(
    { name: 'MoltbotClient', version: '1.0.0' },
    { capabilities: {} },
  );
  await client.connect(clientTransport);

  return {
    server,
    client,
    receiverAddress,
    networkUrl,
    senderSigner,
    hasFundedKeys,
  };
}
