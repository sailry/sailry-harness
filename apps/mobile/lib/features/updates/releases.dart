import 'package:pub_semver/pub_semver.dart';

const releaseRepository = 'sailry/sailry-harness';

final _androidBuildLabel = RegExp(r'Android build ([1-9][0-9]*)');

int? _buildFromLabel(Object? label) {
  if (label is! String) return null;
  final match = _androidBuildLabel.matchAsPrefix(label);
  if (match == null || match.end != label.length) return null;
  return int.tryParse(match[1]!);
}

class Release {
  const Release({
    required this.version,
    required this.download,
    this.buildNumber,
  });

  final Version version;
  final Uri download;
  final int? buildNumber;

  bool newerThan(Version current, int? currentBuildNumber) {
    if (currentBuildNumber == null) return version > current;
    final build = buildNumber;
    return version >= current && build != null && build > currentBuildNumber;
  }

  bool _laterThan(Release other) =>
      version > other.version ||
      version == other.version && (buildNumber ?? 0) > (other.buildNumber ?? 0);
}

/// SemVer precedence excludes Android's independently checked install code.
Version precedence(String value) {
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
  String repository = releaseRepository,
  String tagPrefix = 'v',
  int? currentBuildNumber,
}) {
  if (metadata is! List) {
    throw const FormatException('Release metadata must be a list');
  }
  Release? latest;
  Release? update;
  for (final item in metadata) {
    if (item is! Map<String, dynamic> ||
        item['draft'] is! bool ||
        item['tag_name'] is! String ||
        item['assets'] is! List) {
      throw const FormatException('Invalid release metadata');
    }
    if (item['draft'] == true) continue;
    final tag = item['tag_name'] as String;
    if (!tag.startsWith(tagPrefix)) continue;
    Version version;
    int? buildNumber;
    final bool useAssetBuild;
    try {
      final parsed = Version.parse(tag.substring(tagPrefix.length));
      version = precedence(parsed.toString());
      // Unified tags leave the Android install code on the APK itself.
      useAssetBuild =
          currentBuildNumber != null &&
          tagPrefix == 'v' &&
          parsed.build.isEmpty;
      if (parsed.build.length == 1) {
        buildNumber = int.tryParse(parsed.build.single.toString());
      }
    } on FormatException {
      continue;
    }
    if (currentBuildNumber != null &&
        !useAssetBuild &&
        (buildNumber == null || buildNumber <= 0)) {
      continue;
    }
    // A stable installation never switches to previews automatically.
    if (current.preRelease.isEmpty &&
        (version.preRelease.isNotEmpty || item['prerelease'] == true)) {
      continue;
    }
    // Android is the only mobile distribution target in this release workflow.
    if (target != 'android-arm64') continue;
    // Current releases publish a fixed name; 0.1.0 published a versioned one.
    final names = {
      'Sailry-$target.apk',
      'Sailry-${tag.substring(tagPrefix.length)}-$target.apk',
    };
    for (final asset in item['assets'] as List) {
      if (asset is! Map<String, dynamic>) {
        throw const FormatException('Invalid release asset');
      }
      final name = asset['name'];
      if (!names.contains(name) || asset['state'] != 'uploaded') continue;
      final assetBuild = useAssetBuild
          ? _buildFromLabel(asset['label'])
          : buildNumber;
      if (useAssetBuild && assetBuild == null) continue;
      final expected = [
        ...repository.split('/'),
        'releases',
        'download',
        tag,
        name,
      ];
      final source = asset['browser_download_url'];
      final download = source is String ? Uri.tryParse(source) : null;
      if (download == null ||
          !source.startsWith('https://github.com/') ||
          download.scheme != 'https' ||
          download.host != 'github.com' ||
          download.userInfo.isNotEmpty ||
          download.hasPort ||
          download.hasQuery ||
          download.hasFragment ||
          download.pathSegments.length != expected.length ||
          !expected.indexed.every(
            (segment) => download.pathSegments[segment.$1] == segment.$2,
          ) ||
          asset['size'] is! num ||
          (asset['size'] as num) <= 0) {
        throw const FormatException('Invalid mobile release asset');
      }
      final candidate = Release(
        version: version,
        download: download,
        buildNumber: assetBuild,
      );
      if (latest == null || candidate._laterThan(latest)) {
        latest = candidate;
      }
      if (candidate.newerThan(current, currentBuildNumber) &&
          (update == null || candidate._laterThan(update))) {
        update = candidate;
      }
    }
  }
  return update ?? latest;
}
