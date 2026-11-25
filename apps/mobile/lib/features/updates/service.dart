import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;
import 'package:package_info_plus/package_info_plus.dart';

import 'releases.dart';

enum UpdateResult { unpublished, current, available }

/// Application distribution is independent of the selected execution Node.
class AppUpdates extends ChangeNotifier {
  AppUpdates({
    http.Client? client,
    Future<PackageInfo> Function()? readInfo,
    String? target,
    Uri? source,
    String repository = releaseRepository,
    String tagPrefix = 'v',
    bool requireBuildNumber = false,
    this.timeout = const Duration(seconds: 20),
  }) : _client = client ?? http.Client(),
       _readInfo = readInfo ?? PackageInfo.fromPlatform,
       _target = target ?? (Platform.isAndroid ? 'android-arm64' : 'ios'),
       _repository = repository,
       _tagPrefix = tagPrefix,
       _requireBuildNumber = requireBuildNumber,
       _source =
           source ??
           Uri.https('api.github.com', '/repos/$repository/releases', {
             'per_page': '100',
           });

  final http.Client _client;
  final Future<PackageInfo> Function() _readInfo;
  final String _target;
  final String _repository;
  final String _tagPrefix;
  final bool _requireBuildNumber;
  final Uri _source;
  final Duration timeout;
  PackageInfo? _info;
  bool _closed = false;
  bool checking = false;
  Release? available;

  String? get version => _info?.version;

  Future<UpdateResult?> check() async {
    if (_closed || checking) return null;
    checking = true;
    available = null;
    notifyListeners();
    try {
      final info = _info ?? await _readInfo().timeout(timeout);
      final current = precedence(info.version);
      final currentBuildNumber = _requireBuildNumber
          ? int.tryParse(info.buildNumber)
          : null;
      if (_requireBuildNumber &&
          (currentBuildNumber == null || currentBuildNumber <= 0)) {
        throw const FormatException('Installed mobile build number is invalid');
      }
      if (_closed) return null;
      _info = info;
      if (_target != 'android-arm64') return UpdateResult.unpublished;
      final response = await _client
          .get(
            _source,
            headers: {
              'accept': 'application/vnd.github+json',
              'user-agent': '${info.appName}/${info.version}',
            },
          )
          .timeout(timeout);
      if (_closed) return null;
      if (response.statusCode != 200) {
        throw HttpException('Release check failed (${response.statusCode})');
      }
      if (response.bodyBytes.length > 1024 * 1024) {
        throw const FormatException('Release metadata exceeds its limit');
      }
      final latest = latestRelease(
        jsonDecode(utf8.decode(response.bodyBytes)),
        current: current,
        target: _target,
        repository: _repository,
        tagPrefix: _tagPrefix,
        currentBuildNumber: currentBuildNumber,
      );
      available =
          latest != null && latest.newerThan(current, currentBuildNumber)
          ? latest
          : null;
      return latest == null
          ? UpdateResult.unpublished
          : available == null
          ? UpdateResult.current
          : UpdateResult.available;
    } finally {
      if (!_closed) {
        checking = false;
        notifyListeners();
      }
    }
  }

  @override
  void dispose() {
    _closed = true;
    _client.close();
    super.dispose();
  }
}
