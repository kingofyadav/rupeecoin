// AUTO-GENERATED — DO NOT EDIT BY HAND
// Run `flutter_rust_bridge_codegen generate` from the mobile-wallet/ directory
// to regenerate this file from rust/src/api.rs.
//
// Stub file — provides the same public API surface as the generated file so
// that the rest of the Dart codebase can be analyzed without running codegen.
// The REAL implementation is generated; this stub is for IDE analysis only.

library frb_generated;

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

class WalletInfo {
  final String address;
  final String addressHex;
  final int receiveIndex;
  final String keystorePath;

  const WalletInfo({
    required this.address,
    required this.addressHex,
    required this.receiveIndex,
    required this.keystorePath,
  });
}

abstract class RustLib {
  static Future<void> init() async {}

  static Future<WalletInfo> generateWallet({
    required String passphrase,
    required String keystorePath,
  }) async => throw UnimplementedError('run flutter_rust_bridge_codegen generate');

  static Future<WalletInfo> unlockWallet({
    required String passphrase,
    required String keystorePath,
  }) async => throw UnimplementedError('run flutter_rust_bridge_codegen generate');

  static Future<WalletInfo> advanceAddress({
    required String passphrase,
    required String keystorePath,
  }) async => throw UnimplementedError('run flutter_rust_bridge_codegen generate');

  static Future<bool> checkPassphrase({
    required String passphrase,
    required String keystorePath,
  }) async => throw UnimplementedError('run flutter_rust_bridge_codegen generate');

  static Future<void> changePassphrase({
    required String oldPassphrase,
    required String newPassphrase,
    required String keystorePath,
  }) async => throw UnimplementedError('run flutter_rust_bridge_codegen generate');

  static Future<String> validateAddress({required String address}) async =>
      throw UnimplementedError('run flutter_rust_bridge_codegen generate');
}
