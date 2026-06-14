import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'src/rust/frb_generated.dart';
import 'models/wallet_state.dart';
import 'app.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();

  final state = WalletState();
  await state.init();

  runApp(
    ChangeNotifierProvider.value(
      value: state,
      child: const RupeeCoinWalletApp(),
    ),
  );
}
