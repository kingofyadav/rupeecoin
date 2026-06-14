import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import '../models/wallet_state.dart';

class SettingsScreen extends StatefulWidget {
  const SettingsScreen({super.key});

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  late final TextEditingController _rpcCtrl;
  late final TextEditingController _chainCtrl;
  bool _saved = false;

  @override
  void initState() {
    super.initState();
    final state = context.read<WalletState>();
    _rpcCtrl   = TextEditingController(text: state.rpcUrl);
    _chainCtrl = TextEditingController(text: state.chainId.toString());
  }

  @override
  void dispose() {
    _rpcCtrl.dispose();
    _chainCtrl.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final state = context.read<WalletState>();
    final chainId = int.tryParse(_chainCtrl.text) ?? state.chainId;
    await state.setRpcUrl(_rpcCtrl.text.trim());
    await state.setChainId(chainId);
    if (mounted) setState(() => _saved = true);
    await Future.delayed(const Duration(seconds: 2));
    if (mounted) setState(() => _saved = false);
  }

  @override
  Widget build(BuildContext context) {
    final state = context.watch<WalletState>();
    final cs    = Theme.of(context).colorScheme;

    return Scaffold(
      appBar: AppBar(title: const Text('Settings')),
      body: SafeArea(
        child: ListView(
          padding: const EdgeInsets.all(20),
          children: [
            // Node connection
            _SectionHeader(title: 'Node Connection'),
            const SizedBox(height: 12),
            TextField(
              controller: _rpcCtrl,
              keyboardType: TextInputType.url,
              decoration: const InputDecoration(
                labelText: 'RPC URL',
                hintText: 'http://127.0.0.1:9944',
                prefixIcon: Icon(Icons.dns_rounded),
              ),
            ),
            const SizedBox(height: 12),
            TextField(
              controller: _chainCtrl,
              keyboardType: TextInputType.number,
              inputFormatters: [FilteringTextInputFormatter.digitsOnly],
              decoration: const InputDecoration(
                labelText: 'Chain ID',
                hintText: '1337',
                prefixIcon: Icon(Icons.tag_rounded),
              ),
            ),
            const SizedBox(height: 16),
            ElevatedButton.icon(
              onPressed: _save,
              icon: Icon(_saved ? Icons.check : Icons.save_rounded),
              label: Text(_saved ? 'Saved!' : 'Save & Reconnect'),
              style: _saved
                  ? ElevatedButton.styleFrom(backgroundColor: Colors.greenAccent[700])
                  : null,
            ),

            const SizedBox(height: 32),

            // Node status
            _SectionHeader(title: 'Node Status'),
            const SizedBox(height: 12),
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  children: [
                    _StatusRow(
                      label: 'Connection',
                      value: state.nodeOnline ? 'Online' : 'Offline',
                      valueColor: state.nodeOnline ? Colors.greenAccent : cs.error,
                    ),
                    const Divider(height: 24),
                    _StatusRow(
                      label: 'Block Height',
                      value: state.nodeOnline ? '#${state.nodeHeight}' : '—',
                    ),
                    const Divider(height: 24),
                    _StatusRow(
                      label: 'RPC URL',
                      value: state.rpcUrl,
                      mono: true,
                    ),
                    const Divider(height: 24),
                    _StatusRow(
                      label: 'Chain ID',
                      value: state.chainId.toString(),
                    ),
                  ],
                ),
              ),
            ),

            const SizedBox(height: 32),

            // Wallet info
            _SectionHeader(title: 'Wallet'),
            const SizedBox(height: 12),
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  children: [
                    _StatusRow(
                      label: 'HD Index',
                      value: '#${state.receiveIndex}',
                    ),
                    const Divider(height: 24),
                    _StatusRow(
                      label: 'Keystore',
                      value: 'app-private / rupeecoin_wallet.json',
                      mono: true,
                    ),
                  ],
                ),
              ),
            ),

            const SizedBox(height: 32),

            // About
            _SectionHeader(title: 'About'),
            const SizedBox(height: 12),
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  children: [
                    _StatusRow(label: 'Version', value: '0.1.0-alpha (Phase 10)'),
                    const Divider(height: 24),
                    _StatusRow(label: 'Network', value: 'Zeta Testnet'),
                    const Divider(height: 24),
                    _StatusRow(label: 'Signature', value: 'Dilithium3 (post-quantum)'),
                    const Divider(height: 24),
                    _StatusRow(label: 'KDF', value: 'Argon2id (64 MiB, 3 iter)'),
                    const Divider(height: 24),
                    _StatusRow(label: 'Encryption', value: 'XChaCha20-Poly1305'),
                  ],
                ),
              ),
            ),

            const SizedBox(height: 32),

            // Danger zone
            _SectionHeader(title: 'Danger Zone', color: cs.error),
            const SizedBox(height: 12),
            OutlinedButton.icon(
              onPressed: () => _confirmLock(context),
              icon: Icon(Icons.lock_rounded, color: cs.error),
              label: Text('Lock Wallet', style: TextStyle(color: cs.error)),
              style: OutlinedButton.styleFrom(
                minimumSize: const Size(double.infinity, 44),
                side: BorderSide(color: cs.error.withOpacity(0.4)),
              ),
            ),
            const SizedBox(height: 40),
          ],
        ),
      ),
    );
  }

  void _confirmLock(BuildContext context) {
    showDialog(
      context: context,
      builder: (_) => AlertDialog(
        title: const Text('Lock Wallet?'),
        content: const Text(
          'Your passphrase will be cleared from memory. '
          'You will need to enter it again to access your wallet.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () {
              context.read<WalletState>().lock();
              Navigator.of(context).popUntil((r) => r.isFirst);
            },
            child: Text('Lock', style: TextStyle(color: Theme.of(context).colorScheme.error)),
          ),
        ],
      ),
    );
  }
}

class _SectionHeader extends StatelessWidget {
  final String title;
  final Color? color;
  const _SectionHeader({required this.title, this.color});

  @override
  Widget build(BuildContext context) {
    return Text(
      title,
      style: TextStyle(
        fontSize: 13,
        fontWeight: FontWeight.w600,
        color: color ?? Theme.of(context).colorScheme.onSurface.withOpacity(0.4),
        letterSpacing: 0.8,
      ),
    );
  }
}

class _StatusRow extends StatelessWidget {
  final String label;
  final String value;
  final Color? valueColor;
  final bool mono;

  const _StatusRow({
    required this.label,
    required this.value,
    this.valueColor,
    this.mono = false,
  });

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          width: 100,
          child: Text(label,
              style: TextStyle(
                fontSize: 13,
                color: cs.onSurface.withOpacity(0.5),
              )),
        ),
        Expanded(
          child: Text(
            value,
            style: TextStyle(
              fontSize: 13,
              fontWeight: FontWeight.w500,
              color: valueColor ?? cs.onSurface,
              fontFamily: mono ? 'monospace' : null,
            ),
            textAlign: TextAlign.end,
          ),
        ),
      ],
    );
  }
}
