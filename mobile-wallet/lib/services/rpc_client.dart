import 'dart:convert';
import 'package:http/http.dart' as http;

class UtxoEntry {
  final String txid;
  final int vout;
  final int amountMicro;

  UtxoEntry({required this.txid, required this.vout, required this.amountMicro});

  factory UtxoEntry.fromJson(Map<String, dynamic> j) => UtxoEntry(
        txid: j['txid'] as String,
        vout: j['vout'] as int,
        amountMicro: j['amount_micro'] as int,
      );
}

class NodeStatus {
  final bool online;
  final int height;
  final int mempoolSize;

  NodeStatus({required this.online, required this.height, required this.mempoolSize});
}

class RpcClient {
  final String baseUrl;

  const RpcClient(this.baseUrl);

  Uri _uri(String path, [Map<String, String>? params]) {
    final uri = Uri.parse('$baseUrl$path');
    return params != null ? uri.replace(queryParameters: params) : uri;
  }

  Future<NodeStatus> getStatus() async {
    try {
      final r = await http.get(_uri('/rpc/status')).timeout(const Duration(seconds: 5));
      if (r.statusCode != 200) return NodeStatus(online: false, height: 0, mempoolSize: 0);
      final j = jsonDecode(r.body) as Map<String, dynamic>;
      return NodeStatus(
        online: true,
        height: (j['height'] as num).toInt(),
        mempoolSize: (j['mempool_size'] as num? ?? 0).toInt(),
      );
    } catch (_) {
      return NodeStatus(online: false, height: 0, mempoolSize: 0);
    }
  }

  Future<int> getHeight() async {
    final r = await http.get(_uri('/rpc/height')).timeout(const Duration(seconds: 8));
    _check(r);
    return (jsonDecode(r.body)['height'] as num).toInt();
  }

  Future<List<UtxoEntry>> getUtxos(String addrHex) async {
    final r = await http
        .get(_uri('/rpc/utxos', {'addr': addrHex}))
        .timeout(const Duration(seconds: 8));
    _check(r);
    final list = jsonDecode(r.body) as List<dynamic>;
    return list.map((e) => UtxoEntry.fromJson(e as Map<String, dynamic>)).toList();
  }

  Future<int> getNonce(String addrHex) async {
    final r = await http
        .get(_uri('/rpc/nonce', {'addr': addrHex}))
        .timeout(const Duration(seconds: 8));
    _check(r);
    return (jsonDecode(r.body)['nonce'] as num).toInt();
  }

  Future<String> submitTx(String txHex) async {
    final r = await http
        .post(
          _uri('/rpc/tx'),
          headers: {'Content-Type': 'application/octet-stream'},
          body: txHex,
        )
        .timeout(const Duration(seconds: 15));
    _check(r);
    final j = jsonDecode(r.body) as Map<String, dynamic>;
    if (j.containsKey('error')) throw RpcException(j['error'].toString());
    return j['txid']?.toString() ?? '';
  }

  void _check(http.Response r) {
    if (r.statusCode < 200 || r.statusCode >= 300) {
      throw RpcException('HTTP ${r.statusCode}: ${r.body}');
    }
  }
}

class RpcException implements Exception {
  final String message;
  const RpcException(this.message);
  @override
  String toString() => 'RpcException: $message';
}
