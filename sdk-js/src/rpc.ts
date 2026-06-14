import type { NodeStatus, UtxoEntry, ValidatorInfo } from './types.js';

/** All node RPC endpoints used by the SDK. */
export class RpcClient {
  constructor(private readonly baseUrl: string = 'http://127.0.0.1:9944') {
    this.baseUrl = baseUrl.replace(/\/$/, '');
  }

  private url(path: string): string {
    return `${this.baseUrl}${path}`;
  }

  private async get<T>(path: string): Promise<T> {
    const res = await fetch(this.url(path));
    if (!res.ok) throw new Error(`GET ${path} → ${res.status} ${res.statusText}`);
    return res.json() as Promise<T>;
  }

  /** GET /rpc/height → current chain tip height. */
  async getHeight(): Promise<bigint> {
    const r = await this.get<{ height: number }>('/rpc/height');
    return BigInt(r.height);
  }

  /** GET /rpc/status → comprehensive node snapshot. */
  async getStatus(): Promise<NodeStatus> {
    const r = await this.get<Record<string, unknown>>('/rpc/status');
    return {
      height: BigInt(r['height'] as number),
      slot: BigInt(r['slot'] as number),
      epoch: BigInt(r['epoch'] as number),
      last_justified_epoch: BigInt(r['last_justified_epoch'] as number),
      last_finalized_epoch: BigInt(r['last_finalized_epoch'] as number),
      active_validators: r['active_validators'] as number,
      total_stake: BigInt(r['total_stake'] as number),
      mempool_size: r['mempool_size'] as number,
      pending_privacy_txs: r['pending_privacy_txs'] as number,
      commitment_count: BigInt(r['commitment_count'] as number),
      base_fee: BigInt(r['base_fee'] as number),
    };
  }

  /**
   * GET /rpc/utxos?addr=<hex> → unspent transparent UTXOs for an address.
   * @param addrHex  40-char lowercase hex of the 20-byte address
   */
  async getUtxos(addrHex: string): Promise<UtxoEntry[]> {
    const raw = await this.get<Array<Record<string, unknown>>>(
      `/rpc/utxos?addr=${addrHex}`
    );
    return raw.map(r => ({
      txid: r['txid'] as string,
      vout: r['vout'] as number,
      amount_micro: BigInt(r['amount_micro'] as number),
      amount_rpc: r['amount_rpc'] as number,
    }));
  }

  /**
   * POST /rpc/tx — submit a signed transparent transaction.
   * @param txHex  bincode-serialized Transaction as lowercase hex
   * @returns status string from the node
   */
  async submitTx(txHex: string): Promise<string> {
    const res = await fetch(this.url('/rpc/tx'), {
      method: 'POST',
      headers: { 'Content-Type': 'text/plain' },
      body: txHex,
    });
    if (!res.ok) throw new Error(`POST /rpc/tx → ${res.status} ${res.statusText}`);
    const r = (await res.json()) as { status: string };
    return r.status;
  }

  /**
   * GET /rpc/validator_info?addr=<hex> → validator metrics for an address.
   */
  async getValidatorInfo(addrHex: string): Promise<ValidatorInfo> {
    const r = await this.get<Record<string, unknown>>(
      `/rpc/validator_info?addr=${addrHex}`
    );
    return {
      claimable_rewards: BigInt(r['claimable_rewards'] as number),
      stake: BigInt(r['stake'] as number),
      uptime_score: r['uptime_score'] as number,
      proposal_score: r['proposal_score'] as number,
      total_score: r['total_score'] as number,
      is_active: r['is_active'] as boolean,
      status: r['status'] as string,
    };
  }
}
