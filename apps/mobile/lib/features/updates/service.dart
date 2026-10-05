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
    this.timeout = const Duration(seconds: 20),
  }) : _client = client ?? http.Client(),
       _readInfo = readInfo ?? PackageInfo.fromPlatform,
       _target = target ?? (Platform.isAndroid ? 'android-arm64' : 'ios'),
       _source =
           source ??
           Uri.https('api.github.com', '/repos/$releaseRepository/releases', {
             'per_page': '100',
           });

  final http.Client _client;
  final Future<PackageInfo> Function() _readInfo;
  final String _target;
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
    notifyListeners();
    try {
      final info = _info ?? await _readInfo().timeout(timeout);
      final current = precedence(info.version);
      if (_closed) return null;
      _info = info;
      if (_target != 'android-arm64') return UpdateResult.unpublished;
      final response = await _client
          .get(
            _source,
            headers: {
              'accept': 'application/vnd.github+json',
              'user-agent': 'Sailry-Harness/${info.version}',
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
      );
      available = latest != null && latest.version > current ? latest : null;
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
