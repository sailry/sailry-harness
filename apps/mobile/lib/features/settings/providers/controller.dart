import 'package:flutter/foundation.dart';

import '../../../runtime/json.dart';
import '../../../runtime/session.dart' show HostConnection;
import 'rules.dart';

/// One provider being edited on its execution Node. Saves are serialized and
/// replace the local copy with the Node's revision.
class ProviderController extends ChangeNotifier {
  ProviderController(this.host, Json provider) : _provider = provider;

  final HostConnection host;
  Json _provider;
  bool _busy = false;

  Json get provider => _provider;
  bool get busy => _busy;
  String get id => text(_provider['id']);
  int get revision => number(_provider['revision']).toInt();

  /// Lists providers; Desktop-created presets and logins are included.
  static Future<List<Json>> list(HostConnection host) async =>
      objects((await host.command('list_providers'))['data']);

  /// Saves a new API-key provider and returns the Node's copy.
  static Future<Json> create(
    HostConnection host,
    Json provider, {
    String? secret,
  }) async => object(
    (await host.command(
      'save_provider',
      saveRequest(provider, secret: secret),
    ))['data'],
  );

  /// Saves an edited copy of [provider]. Returns false when another save is
  /// still running; failures are rethrown for the caller to report.
  Future<bool> save(Json next, {String? secret}) => _run(() async {
    final output = await host.command(
      'save_provider',
      saveRequest({...next, 'revision': revision}, secret: secret),
    );
    _provider = object(output['data']);
  });

  /// Reads the stored API key for display only; callers must not keep it.
  Future<String> readKey() async => text(
    (await host.command('read_provider_key', {
      'provider': id,
      'expected_revision': revision,
    }))['data'],
  );

  Future<bool> remove() => _run(
    () => host.command('remove_provider', {
      'provider': id,
      'expected_revision': revision,
    }),
  );

  /// Models reported by the saved provider revision.
  Future<List<Json>> discover() async {
    final output = await host.command('discover_models', {
      'kind': 'saved',
      'data': {'provider': id, 'expected_revision': revision},
    });
    return objects(object(output['data'])['models']);
  }

  /// Best-effort catalog references for [ids]; a missing or failed catalog
  /// leaves completion to the Node when the provider is saved.
  Future<Map<String, Json>> references(List<String> ids) async {
    final result = <String, Json>{};
    try {
      for (var start = 0; start < ids.length; start += 100) {
        final chunk = ids.skip(start).take(100).toList();
        Object? revision;
        Object? after;
        for (var page = 0; page < 32; page++) {
          final output = await host.command('read_model_catalog', {
            'provider': catalogProvider(text(_provider['endpoint'])),
            'ids': chunk,
            'revision': revision,
            'after': after,
            'limit': 100,
          });
          final data = object(output['data']);
          for (final model in objects(data['models'])) {
            result.putIfAbsent(text(model['id']), () => model);
          }
          revision = data['revision'];
          after = data['next'];
          if (after == null) break;
        }
      }
    } catch (_) {
      // Node catalog completion on save covers what this lookup missed.
    }
    return result;
  }

  Future<bool> _run(Future<void> Function() action) async {
    if (_busy) return false;
    _busy = true;
    notifyListeners();
    try {
      await action();
      return true;
    } finally {
      _busy = false;
      notifyListeners();
    }
  }
}

/// Catalog provider for well-known endpoints, matching Node completion.
String? catalogProvider(String endpoint) =>
    switch (endpoint.trim().replaceAll(RegExp(r'/+$'), '')) {
      'https://api.openai.com/v1' => 'openai',
      'https://api.anthropic.com' => 'anthropic',
      'https://generativelanguage.googleapis.com/v1beta' => 'google',
      'https://api.deepseek.com' || 'https://api.deepseek.com/v1' => 'deepseek',
      _ => null,
    };
