import 'package:flutter_rust_bridge_hooks/flutter_rust_bridge_hooks.dart';
import 'package:code_assets/code_assets.dart';

import 'speech.dart' as speech;

void main(List<String> args) async {
  await build(args, (input, output) async {
    // Mobile owns bundled native builds. Host-side contract tests load the
    // workspace's explicit Cargo artifact and must not rebuild it per runner.
    if (!input.config.buildCodeAssets ||
        ![OS.iOS, OS.android].contains(input.config.code.targetOS)) {
      return;
    }
    await FlutterRustBridgeNativeAssetsBuilder(
      cratePath: '..',
      assetName: 'frb_generated.io.dart',
      extraCargoBuildArgs: ['--locked'],
      // Hooks run outside Xcode's environment. Keep Rust and C dependencies
      // on Flutter's deployment target rather than their different defaults.
      extraCargoEnvironmentVariables: {
        if (input.config.code.targetOS == OS.iOS)
          'IPHONEOS_DEPLOYMENT_TARGET': input.config.code.iOS.targetVersion
              .toString(),
      },
    ).run(input: input, output: output);
    await speech.bundle(input, output);
  });
}
