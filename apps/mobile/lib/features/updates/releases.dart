import 'package:pub_semver/pub_semver.dart';

const releaseRepository = 'sailry/sailry-harness';

class Release {
  const Release({required this.version, required this.download});

  final Version version;
  final Uri download;
}

/// App updates follow SemVer precedence, not pub's build-number ordering.
Version releaseVersion(String value) {
  final version = Version.parse(value);
  return Version(
    version.major,
    version.minor,
    version.patch,
    pre: version.preRelease.isEmpty ? null : version.preRelease.join('.'),
  );
}

Release? latestRelease(
  Object? metadata, {
  required Version current,
  required String target,
}) {
  if (metadata is! List) {
    throw const FormatException('Release metadata must be a list');
  }
  Release? latest;
  for (final item in metadata) {
    if (item is! Map<String, dynamic> ||
        item['draft'] is! bool ||
        item['tag_name'] is! String ||
        item['assets'] is! List) {
      throw const FormatException('Invalid release metadata');
    }
    if (item['draft'] == true) continue;
    final tag = item['tag_name'] as String;
    if (!tag.startsWith('v')) continue;
    Version version;
    try {
      version = releaseVersion(tag.substring(1));
    } on FormatException {
      continue;
    }
    // A stable installation never switches to previews automatically.
    if (current.preRelease.isEmpty &&
        (version.preRelease.isNotEmpty || item['prerelease'] == true)) {
      continue;
    }
    // Android is the only mobile distribution target in this release workflow.
    if (target != 'android-arm64') continue;
    final name = 'Sailry-${tag.substring(1)}-$target.apk';
    final expected = Uri.https(
      'github.com',
      '/$releaseRepository/releases/download/$tag/$name',
    );
    for (final asset in item['assets'] as List) {
      if (asset is! Map<String, dynamic>) {
        throw const FormatException('Invalid release asset');
      }
      if (asset['name'] != name || asset['state'] != 'uploaded') continue;
      if (asset['browser_download_url'] != expected.toString() ||
          asset['size'] is! num ||
          (asset['size'] as num) <= 0) {
        throw const FormatException('Invalid mobile release asset');
      }
      if (latest == null || version > latest.version) {
        latest = Release(version: version, download: expected);
      }
    }
  }
  return latest;
}
