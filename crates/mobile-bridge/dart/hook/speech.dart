import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:flutter_rust_bridge_hooks/flutter_rust_bridge_hooks.dart';

/// Sherpa supports shared linking on mobile. Cargo places these dependencies
/// beside the bridge, but FRB registers only the bridge itself as a code asset.
Future<void> bundle(BuildInput input, BuildOutputBuilder output) async {
  final bridge = output.build().assets.code.single;
  final ios = input.config.code.targetOS == OS.iOS;
  final libraries = ios
      ? ['libsherpa-onnx-c-api.dylib']
      : ['libsherpa-onnx-c-api.so', 'libonnxruntime.so'];
  for (final name in libraries) {
    final source = File.fromUri(bridge.file!.resolve(name));
    if (!await source.exists()) {
      throw StateError(
        'Speech runtime library was not produced: ${source.path}',
      );
    }
    output.dependencies.add(source.uri);
    var bundled = source;
    if (ios && input.config.code.iOS.targetSdk == IOSSdk.iPhoneSimulator) {
      // The upstream simulator framework is universal. Each native-assets
      // hook must supply only its requested architecture; Flutter combines
      // target slices and rewrites dependent install names during packaging.
      final architecture = switch (input.config.code.targetArchitecture) {
        Architecture.arm64 => 'arm64',
        Architecture.x64 => 'x86_64',
        final value => throw StateError('Unsupported iOS architecture: $value'),
      };
      bundled = File.fromUri(input.outputDirectory.resolve('speech/$name'));
      await bundled.parent.create(recursive: true);
      final result = await Process.run('xcrun', [
        'lipo',
        source.path,
        '-thin',
        architecture,
        '-output',
        bundled.path,
      ]);
      if (result.exitCode != 0) {
        throw StateError(
          'Speech architecture extraction failed: ${result.stderr}',
        );
      }
    }
    output.assets.code.add(
      CodeAsset(
        package: input.packageName,
        name: 'speech/$name',
        linkMode: DynamicLoadingBundled(),
        file: bundled.uri,
      ),
    );
  }
}
