import 'dart:convert';

import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:sailry_mobile/features/updates/releases.dart';
import 'package:sailry_mobile/features/updates/service.dart';

Map<String, dynamic> release(
  String version, {
  bool draft = false,
  String target = 'android-arm64',
  bool versioned = false,
  String repository = releaseRepository,
  String tagPrefix = 'v',
}) {
  final tag = '$tagPrefix$version';
  final name = versioned ? 'Sailry-$version-$target.apk' : 'Sailry-$target.apk';
  return {
    'tag_name': tag,
    'draft': draft,
    'prerelease': version.contains('-'),
    'assets': [
      {
        'name': name,
        'state': 'uploaded',
        'size': 1024,
        'browser_download_url':
            'https://github.com/$repository/releases/download/$tag/$name',
      },
    ],
  };
}

PackageInfo info(String version, {String buildNumber = '1'}) => PackageInfo(
  appName: 'Sailry',
  packageName: 'com.sailry.sailry_mobile',
  version: version,
  buildNumber: buildNumber,
);

AppUpdates updates({
  String current = '0.1.0-alpha.1',
  List<Map<String, dynamic>> releases = const [],
  MockClientHandler? request,
}) => AppUpdates(
  client: MockClient(
    request ?? (_) async => http.Response(jsonEncode(releases), 200),
  ),
  target: 'android-arm64',
  readInfo: () async => info(current),
);
