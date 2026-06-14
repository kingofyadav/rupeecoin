import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../models/wallet_state.dart';

class WelcomeScreen extends StatefulWidget {
  const WelcomeScreen({super.key});

  @override
  State<WelcomeScreen> createState() => _WelcomeScreenState();
}

class _WelcomeScreenState extends State<WelcomeScreen>
    with SingleTickerProviderStateMixin {
  late final TabController _tabs;
  bool _loading = false;
  String? _error;

  // Create tab
  final _createPassCtrl  = TextEditingController();
  final _confirmPassCtrl = TextEditingController();

  // Unlock tab
  final _unlockPassCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    _tabs = TabController(length: 2, vsync: this);
  }

  @override
  void dispose() {
    _tabs.dispose();
    _createPassCtrl.dispose();
    _confirmPassCtrl.dispose();
    _unlockPassCtrl.dispose();
    super.dispose();
  }

  Future<void> _createWallet() async {
    final pass    = _createPassCtrl.text;
    final confirm = _confirmPassCtrl.text;
    if (pass.length < 8) {
      _setError('Passphrase must be at least 8 characters');
      return;
    }
    if (pass != confirm) {
      _setError('Passphrases do not match');
      return;
    }
    await _run(() => context.read<WalletState>().generateWallet(pass));
  }

  Future<void> _unlockWallet() async {
    final pass = _unlockPassCtrl.text;
    if (pass.isEmpty) {
      _setError('Enter your passphrase');
      return;
    }
    await _run(() => context.read<WalletState>().unlockWallet(pass));
  }

  Future<void> _run(Future<void> Function() fn) async {
    setState(() { _loading = true; _error = null; });
    try {
      await fn();
    } catch (e) {
      _setError(e.toString());
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  void _setError(String msg) {
    if (mounted) setState(() => _error = msg);
  }

  @override
  Widget build(BuildContext context) {
    final cs = Theme.of(context).colorScheme;

    return Scaffold(
      body: SafeArea(
        child: SingleChildScrollView(
          padding: const EdgeInsets.symmetric(horizontal: 24, vertical: 32),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              const SizedBox(height: 32),
              // Logo
              Container(
                width: 72,
                height: 72,
                decoration: BoxDecoration(
                  color: cs.primary.withOpacity(0.12),
                  borderRadius: BorderRadius.circular(20),
                ),
                child: Icon(Icons.account_balance_wallet_rounded,
                    size: 40, color: cs.primary),
              ),
              const SizedBox(height: 20),
              Text('RupeeCoin Wallet',
                  style: Theme.of(context)
                      .textTheme
                      .headlineSmall
                      ?.copyWith(fontWeight: FontWeight.bold)),
              const SizedBox(height: 6),
              Text('Post-quantum · Privacy-native',
                  style: Theme.of(context)
                      .textTheme
                      .bodyMedium
                      ?.copyWith(color: cs.onSurface.withOpacity(0.5))),
              const SizedBox(height: 40),

              // Tabs
              Container(
                decoration: BoxDecoration(
                  color: const Color(0xFF161B22),
                  borderRadius: BorderRadius.circular(10),
                ),
                child: TabBar(
                  controller: _tabs,
                  indicator: BoxDecoration(
                    color: cs.primary,
                    borderRadius: BorderRadius.circular(10),
                  ),
                  indicatorSize: TabBarIndicatorSize.tab,
                  labelColor: Colors.white,
                  unselectedLabelColor: cs.onSurface.withOpacity(0.5),
                  tabs: const [
                    Tab(text: 'Create Wallet'),
                    Tab(text: 'Unlock'),
                  ],
                ),
              ),
              const SizedBox(height: 24),

              SizedBox(
                height: 260,
                child: TabBarView(
                  controller: _tabs,
                  children: [
                    _CreateTab(
                      passCtrl: _createPassCtrl,
                      confirmCtrl: _confirmPassCtrl,
                      onSubmit: _loading ? null : _createWallet,
                    ),
                    _UnlockTab(
                      passCtrl: _unlockPassCtrl,
                      onSubmit: _loading ? null : _unlockWallet,
                    ),
                  ],
                ),
              ),

              if (_loading) ...[
                const SizedBox(height: 24),
                const CircularProgressIndicator(),
              ],
              if (_error != null) ...[
                const SizedBox(height: 16),
                Container(
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: cs.error.withOpacity(0.1),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: cs.error.withOpacity(0.3)),
                  ),
                  child: Text(_error!,
                      style: TextStyle(color: cs.error, fontSize: 13),
                      textAlign: TextAlign.center),
                ),
              ],

              const SizedBox(height: 32),
              Text('Keystore stored in app private directory',
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                        color: cs.onSurface.withOpacity(0.3),
                      ),
                  textAlign: TextAlign.center),
            ],
          ),
        ),
      ),
    );
  }
}

class _CreateTab extends StatefulWidget {
  final TextEditingController passCtrl;
  final TextEditingController confirmCtrl;
  final VoidCallback? onSubmit;

  const _CreateTab({
    required this.passCtrl,
    required this.confirmCtrl,
    required this.onSubmit,
  });

  @override
  State<_CreateTab> createState() => _CreateTabState();
}

class _CreateTabState extends State<_CreateTab> {
  bool _obscure1 = true;
  bool _obscure2 = true;

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        TextField(
          controller: widget.passCtrl,
          obscureText: _obscure1,
          decoration: InputDecoration(
            hintText: 'Passphrase (min 8 chars)',
            suffixIcon: IconButton(
              icon: Icon(_obscure1 ? Icons.visibility_off : Icons.visibility),
              onPressed: () => setState(() => _obscure1 = !_obscure1),
            ),
          ),
        ),
        const SizedBox(height: 12),
        TextField(
          controller: widget.confirmCtrl,
          obscureText: _obscure2,
          decoration: InputDecoration(
            hintText: 'Confirm passphrase',
            suffixIcon: IconButton(
              icon: Icon(_obscure2 ? Icons.visibility_off : Icons.visibility),
              onPressed: () => setState(() => _obscure2 = !_obscure2),
            ),
          ),
        ),
        const SizedBox(height: 20),
        ElevatedButton(
          onPressed: widget.onSubmit,
          child: const Text('Create New Wallet'),
        ),
      ],
    );
  }
}

class _UnlockTab extends StatefulWidget {
  final TextEditingController passCtrl;
  final VoidCallback? onSubmit;

  const _UnlockTab({required this.passCtrl, required this.onSubmit});

  @override
  State<_UnlockTab> createState() => _UnlockTabState();
}

class _UnlockTabState extends State<_UnlockTab> {
  bool _obscure = true;

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        TextField(
          controller: widget.passCtrl,
          obscureText: _obscure,
          autofillHints: const [AutofillHints.password],
          decoration: InputDecoration(
            hintText: 'Enter your passphrase',
            suffixIcon: IconButton(
              icon: Icon(_obscure ? Icons.visibility_off : Icons.visibility),
              onPressed: () => setState(() => _obscure = !_obscure),
            ),
          ),
          onSubmitted: (_) => widget.onSubmit?.call(),
        ),
        const SizedBox(height: 20),
        ElevatedButton(
          onPressed: widget.onSubmit,
          child: const Text('Unlock Wallet'),
        ),
      ],
    );
  }
}
