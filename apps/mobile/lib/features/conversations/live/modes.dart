import 'dart:convert';

import 'package:flutter/widgets.dart';

import '../../../runtime/json.dart';
import '../../../runtime/session.dart';

class ConversationMode {
  ConversationMode(this.package, this.declaration, {required this.enabled});

  final String package;
  final Map<String, dynamic> declaration;
  final bool enabled;

  String get key => '$package/${declaration['id']}';
  String get option => text(declaration['option']);
  String get base =>
      object(declaration['permissions'])['write'] == true ? 'code' : 'plan';

  bool matches(Map<String, dynamic> config) =>
      object(object(config['options'])[package])[option] == declaration['id'];

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

/// A control-owned metadata projection; Node owns effective configuration.
class ConversationModes extends ChangeNotifier {
  ConversationModes(this.host) {
    host.addListener(_changed);
    _changed();
  }

  final HostConnection host;
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
          name,
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

  List<ConversationMode> choices(Map<String, dynamic> config) => !ready
      ? []
      : [
          for (final mode in _modes)
            if (mode.enabled || mode.matches(config)) mode,
        ];

  String? selected(Map<String, dynamic> config) {
    if (!ready) return null;
    final selected = _modes.where((mode) => mode.matches(config)).toList();
    if (selected.length > 1) return null;
    if (selected.isNotEmpty) {
      final mode = selected.single;
      if (mode.base == 'plan' && config['mode'] != 'plan') return null;
      return mode.key;
    }
    return text(config['mode'], 'code');
  }

  Map<String, dynamic>? select(Map<String, dynamic> config, String key) {
    if (!ready || !host.connected) return null;
    final selected = _modes
        .where((mode) => mode.key == key && mode.enabled)
        .firstOrNull;
    if (selected == null && key != 'code' && key != 'plan') return null;
    final options = {
      for (final entry in object(config['options']).entries)
        entry.key: object(entry.value),
    };
    for (final mode in _modes) {
      if (mode.matches(config)) options[mode.package]![mode.option] = '';
    }
    if (selected != null) {
      options.putIfAbsent(selected.package, () => {})[selected.option] =
          selected.declaration['id'];
    }
    return {...config, 'mode': selected?.base ?? key, 'options': options};
  }

  @override
  void dispose() {
    _disposed = true;
    _generation++;
    host.removeListener(_changed);
    super.dispose();
  }
}
