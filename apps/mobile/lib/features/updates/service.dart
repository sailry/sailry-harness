import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

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
    this._tagPrefix = 'v',
    this._requireBuildNumber = false,
    this.timeout = const Duration(seconds: 20),
  }) : _client = client ?? http.Client(),
       _readInfo = readInfo ?? PackageInfo.fromPlatform,
       _target = target ?? (Platform.isAndroid ? 'android-arm64' : 'ios'),
       _repository = repository,
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
      final code = _repository == 'sailry/sailry-code';
      final candidates = <Release>[];
      final pageLimit = code ? 10 : 1;
      for (var page = 1; page <= pageLimit; page++) {
        final source = code
            ? _source.replace(
                queryParameters: {
                  ..._source.queryParameters,
                  'per_page': '100',
                  'page': '$page',
                },
              )
            : _source;
        final metadata = await _releasePage(source, {
          'accept': 'application/vnd.github+json',
          'user-agent': '${info.appName}/${info.version}',
        }, code: code);
        if (_closed) return null;
        final candidate = latestRelease(
          metadata,
          current: current,
          target: _target,
          repository: _repository,
          tagPrefix: _tagPrefix,
          currentBuildNumber: currentBuildNumber,
        );
        if (candidate != null) candidates.add(candidate);
        if (!code || metadata is List && metadata.length < 100) break;
        if (page == pageLimit) {
          throw const FormatException(
            'Release discovery exceeds its page limit',
          );
        }
      }
      candidates.sort((a, b) {
        final aUpdate = a.newerThan(current, currentBuildNumber);
        final bUpdate = b.newerThan(current, currentBuildNumber);
        if (aUpdate != bUpdate) return aUpdate ? -1 : 1;
        final version = b.version.compareTo(a.version);
        return version != 0
            ? version
            : (b.buildNumber ?? 0).compareTo(a.buildNumber ?? 0);
      });
      final latest = candidates.firstOrNull;
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

  Future<Object?> _releasePage(
    Uri source,
    Map<String, String> headers, {
    required bool code,
  }) async {
    if (!code) {
      final response = await _client
          .get(source, headers: headers)
          .timeout(timeout);
      if (_closed) return null;
      if (response.statusCode != 200) {
        throw HttpException('Release check failed (${response.statusCode})');
      }
      if (response.bodyBytes.length > 1024 * 1024) {
        throw const FormatException('Release metadata exceeds its limit');
      }
      return jsonDecode(utf8.decode(response.bodyBytes));
    }
    const maxPage = 8 * 1024 * 1024;
    final request = http.Request('GET', source)
      ..headers.addAll(headers)
      ..followRedirects = false;
    final response = await _client.send(request).timeout(timeout);
    if (response.statusCode != 200) {
      await response.stream.listen(null).cancel();
      throw HttpException('Release check failed (${response.statusCode})');
    }
    if ((response.contentLength ?? 0) > maxPage) {
      await response.stream.listen(null).cancel();
      throw const FormatException('Release metadata exceeds its limit');
    }
    final bytes = BytesBuilder(copy: false);
    await for (final chunk in response.stream.timeout(timeout)) {
      if (_closed) return null;
      if (bytes.length + chunk.length > maxPage) {
        throw const FormatException('Release metadata exceeds its limit');
      }
      bytes.add(chunk);
    }
    return jsonDecode(utf8.decode(bytes.takeBytes()));
  }

  @override
  void dispose() {
    _closed = true;
    _client.close();
    super.dispose();
  }
}
