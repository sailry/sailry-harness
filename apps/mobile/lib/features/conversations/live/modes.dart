import 'dart:convert';

import 'package:flutter/widgets.dart';
import 'package:sailry_bridge/api/rules.dart' as rules;

import '../../../runtime/json.dart';
import '../../../runtime/session.dart';

class ConversationMode {
  ConversationMode(this.reference, this.declaration, {required this.enabled});

  final Map<String, dynamic> reference;
  String get package => text(reference['name']);
  final Map<String, dynamic> declaration;
  final bool enabled;

  String get key => '$package/${declaration['id']}';
  Map<String, dynamic> get wire => {
    'package': reference,
    'declaration': declaration,
    'enabled': enabled,
  };

  String label(Locale locale) {
    final display = object(declaration['display']);
    final locales = object(display['locales']);
    final tag = switch (locale.languageCode) {
      'zh' =>
        locale.scriptCode == 'Hant' ||
                const ['TW', 'HK', 'MO'].contains(locale.countryCode)
            ? 'zh-TW'
            : 'zh-CN',
      'pt' => 'pt-BR',
      _ => locale.toLanguageTag(),
    };
    return text(
      locales[tag],
      text(locales[locale.languageCode], text(display['label'])),
    );
  }
}

typedef ModeProjection =
    Map<String, dynamic> Function(
      Map<String, dynamic> config,
      List<ConversationMode> modes,
    );
typedef ModeSelection =
    Map<String, dynamic>? Function(
      Map<String, dynamic> config,
      List<ConversationMode> modes,
      String key,
    );

Map<String, dynamic> _projectModes(
  Map<String, dynamic> config,
  List<ConversationMode> modes,
) => object(
  jsonDecode(
    rules.projectModes(
      config: jsonEncode(config),
      modes: jsonEncode(modes.map((mode) => mode.wire).toList()),
    ),
  ),
);

Map<String, dynamic>? _selectMode(
  Map<String, dynamic> config,
  List<ConversationMode> modes,
  String key,
) {
  final selected = rules.selectMode(
    config: jsonEncode(config),
    modes: jsonEncode(modes.map((mode) => mode.wire).toList()),
    key: key,
  );
  return selected == null ? null : object(jsonDecode(selected));
}

/// A control-owned metadata projection; Node owns effective configuration.
class ConversationModes extends ChangeNotifier {
  ConversationModes(this.host, {ModeProjection? project, ModeSelection? select})
    : _project = project ?? _projectModes,
      _select = select ?? _selectMode {
    host.addListener(_changed);
    _changed();
  }

  final HostConnection host;
  final ModeProjection _project;
  final ModeSelection _select;
  final Map<String, (String, List<ConversationMode>)> _cache = {};
  List<ConversationMode> _modes = [];
  String? _inventory;
  int _generation = 0;
  bool _disposed = false;
  bool ready = false;
  Object? error;
  Future<void> settled = Future.value();

  static String _identity(Map<String, dynamic> summary) => jsonEncode([
    summary['name'],
    summary['revision'],
    summary['digest'],
    summary['settings_revision'],
    summary['enabled'],
  ]);

  void _changed() {
    final summaries = objects(host.snapshot['plugins']);
    final inventory = jsonEncode([
      host.connected,
      summaries.map(_identity).toList(),
    ]);
    if (_inventory == inventory) return;
    _inventory = inventory;
    final generation = ++_generation;
    final names = summaries.map((summary) => text(summary['name'])).toSet();
    _cache.removeWhere((name, _) => !names.contains(name));
    ready = false;
    error = null;
    notifyListeners();
    if (host.connected) settled = _load(summaries, generation);
  }

  Future<List<ConversationMode>> _read(Map<String, dynamic> summary) async {
    final name = text(summary['name']), identity = _identity(summary);
    final cached = _cache[name];
    if (cached != null && cached.$1 == identity) return cached.$2;
    final response = await host.command('read_plugin', {'name': name});
    final info = object(response['data']);
    if (_identity(object(info['summary'])) != identity) {
      throw const CommandFailure('revision_conflict');
    }
    final extension = object(info['extension']);
    if (extension['scope'] == 'desktop') return [];
    return [
      for (final declaration in objects(extension['conversation_modes']))
        ConversationMode(
          {
            for (final key in ['name', 'digest', 'settings_revision'])
              key: summary[key],
          },
          declaration,
          enabled: summary['enabled'] == true,
        ),
    ];
  }

  Future<void> _load(
    List<Map<String, dynamic>> summaries,
    int generation,
  ) async {
    try {
      final packages = await Future.wait(summaries.map(_read));
      if (_disposed || generation != _generation) return;
      for (var index = 0; index < summaries.length; index++) {
        final summary = summaries[index];
        _cache[text(summary['name'])] = (_identity(summary), packages[index]);
      }
      _modes = packages.expand((modes) => modes).toList();
      ready = true;
    } catch (failure) {
      if (_disposed || generation != _generation) return;
      error = failure;
    }
    notifyListeners();
  }

  List<ConversationMode> choices(Map<String, dynamic> config) {
    if (!ready) return [];
    final keys = _project(config, _modes)['choices'] as List;
    return [
      for (final key in keys) _modes.firstWhere((mode) => mode.key == key),
    ];
  }

  String? selected(Map<String, dynamic> config) =>
      ready ? _project(config, _modes)['selected'] as String? : null;

  Map<String, dynamic>? select(Map<String, dynamic> config, String key) =>
      ready && host.connected ? _select(config, _modes, key) : null;

  @override
  void dispose() {
    _disposed = true;
    _generation++;
    host.removeListener(_changed);
    super.dispose();
  }
}
