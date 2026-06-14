import 'package:flutter/foundation.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:path_provider/path_provider.dart';
import '../src/rust/frb_generated.dart' show RustLib;
import '../src/rust/api.dart' as rustApi;
import '../services/rpc_client.dart';

const _prefRpcUrl  = 'rpc_url';
const _prefChainId = 'chain_id';
const _defaultRpc  = 'http://127.0.0.1:9944';
const _defaultChainId = 1337;
const _microPerCoin = 1_000_000;

class WalletState extends ChangeNotifier {
  // ── Settings ──────────────────────────────────────────────────────────────
  String _rpcUrl  = _defaultRpc;
  int    _chainId = _defaultChainId;

  String get rpcUrl  => _rpcUrl;
  int    get chainId => _chainId;

  RpcClient get _rpc => RpcClient(_rpcUrl);

  // ── Wallet identity ───────────────────────────────────────────────────────
  String? _address;
  String? _addressHex;
  int     _receiveIndex = 0;
  String? _keystorePath;

  String? get address       => _address;
  String? get addressHex    => _addressHex;
  int     get receiveIndex  => _receiveIndex;
  bool    get isUnlocked    => _address != null;

  // ── Balance ───────────────────────────────────────────────────────────────
  int  _balanceMicro = 0;
  int  _utxoCount    = 0;
  bool _loadingBalance = false;
  String? _balanceError;

  double get balanceCoin    => _balanceMicro / _microPerCoin;
  int    get utxoCount      => _utxoCount;
  bool   get loadingBalance => _loadingBalance;
  String? get balanceError  => _balanceError;

  // ── Node status ───────────────────────────────────────────────────────────
  bool _nodeOnline = false;
  int  _nodeHeight = 0;

  bool get nodeOnline => _nodeOnline;
  int  get nodeHeight => _nodeHeight;

  // ── Passphrase cache (in-memory only, never persisted) ────────────────────
  String? _passphrase;

  // ── Init ──────────────────────────────────────────────────────────────────

  Future<void> init() async {
    final prefs = await SharedPreferences.getInstance();
    _rpcUrl  = prefs.getString(_prefRpcUrl)  ?? _defaultRpc;
    _chainId = prefs.getInt(_prefChainId)    ?? _defaultChainId;
    notifyListeners();
  }

  // ── Settings ──────────────────────────────────────────────────────────────

  Future<void> setRpcUrl(String url) async {
    _rpcUrl = url.trimRight().replaceAll(RegExp(r'/$'), '');
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_prefRpcUrl, _rpcUrl);
    notifyListeners();
    await refreshNodeStatus();
  }

  Future<void> setChainId(int id) async {
    _chainId = id;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setInt(_prefChainId, id);
    notifyListeners();
  }

  // ── Wallet create / unlock ─────────────────────────────────────────────────

  Future<String> defaultKeystorePath() async {
    final dir = await getApplicationDocumentsDirectory();
    return '${dir.path}/rupeecoin_wallet.json';
  }

  Future<void> generateWallet(String passphrase) async {
    final path = await defaultKeystorePath();
    final info = rustApi.generateWallet(passphrase: passphrase, keystorePath: path);
    _applyInfo(info, passphrase);
    await refreshBalance();
  }

  Future<void> unlockWallet(String passphrase) async {
    final path = await defaultKeystorePath();
    final info = rustApi.unlockWallet(passphrase: passphrase, keystorePath: path);
    _applyInfo(info, passphrase);
    await refreshBalance();
  }

  void lock() {
    _address    = null;
    _addressHex = null;
    _passphrase = null;
    _balanceMicro = 0;
    _utxoCount  = 0;
    notifyListeners();
  }

  Future<void> advanceAddress() async {
    if (_keystorePath == null || _passphrase == null) return;
    final info = rustApi.advanceAddress(
      passphrase: _passphrase!,
      keystorePath: _keystorePath!,
    );
    _applyInfo(info, _passphrase!);
    await refreshBalance();
  }

  void _applyInfo(WalletInfo info, String passphrase) {
    _address      = info.address;
    _addressHex   = info.addressHex;
    _receiveIndex = info.receiveIndex;
    _keystorePath = info.keystorePath;
    _passphrase   = passphrase;
    notifyListeners();
  }

  // ── Balance & node ─────────────────────────────────────────────────────────

  Future<void> refreshBalance() async {
    if (_addressHex == null) return;
    _loadingBalance = true;
    _balanceError   = null;
    notifyListeners();
    try {
      final utxos = await _rpc.getUtxos(_addressHex!);
      _balanceMicro = utxos.fold(0, (s, u) => s + u.amountMicro);
      _utxoCount    = utxos.length;
    } catch (e) {
      _balanceError = e.toString();
    } finally {
      _loadingBalance = false;
      notifyListeners();
    }
    await refreshNodeStatus();
  }

  Future<void> refreshNodeStatus() async {
    try {
      final s  = await _rpc.getStatus();
      _nodeOnline = s.online;
      _nodeHeight = s.height;
    } catch (_) {
      _nodeOnline = false;
    }
    notifyListeners();
  }
}
