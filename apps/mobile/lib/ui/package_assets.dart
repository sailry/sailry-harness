import 'package:flutter/services.dart';

/// The product owns the root bundle; the shared shell owns these package assets.
class PackageAssetBundle extends CachingAssetBundle {
  PackageAssetBundle(this.package, {AssetBundle? source})
    : _source = source ?? rootBundle;

  final String package;
  final AssetBundle _source;

  @override
  Future<ByteData> load(String key) => _source.load(
    key.startsWith('assets/') || key.startsWith('third_party_licenses/')
        ? 'packages/$package/$key'
        : key,
  );
}
