import 'package:flutter/foundation.dart';

import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import 'team.dart';

enum _Stage { create, team, config, ready }

/// Captures a new session's resources until Node admission is resolved.
class CreationDraft extends ChangeNotifier {
  CreationDraft(this.host, {String? initialProject})
    : _projectId = initialProject == '' ? null : initialProject {
    _sync();
    host.addListener(_changed);
  }

  final HostConnection host;

  /// Generic conversations start outside projects; explicit IDs retain their scope.
  String? _projectId;
  Map<String, dynamic>? get project => _projectId == null
      ? null
      : objects(
              host.snapshot['projects'],
            ).where((item) => item['id'] == _projectId).firstOrNull ??
            {'id': _projectId};

  set project(Map<String, dynamic>? value) {
    if (!locationEditable) return;
    _projectId = value?['id'] as String?;
    worktree = worktrees.firstOrNull;
    notifyListeners();
  }

  Map<String, dynamic>? worktree;
  Map<String, dynamic> config = {};
  Map<String, dynamic>? _created;
  Map<String, dynamic>? _admitted;
  Map<String, dynamic>? _team;
  final _models = <String, dynamic>{};
  _Stage _stage = _Stage.create;
  bool _leadChosen = false;
  bool _configurationChanged = false;
  String? _pendingRequest;
  bool _busy = false;
  bool _uncertain = false;
  bool attaching = false;
  bool _defaultsCaptured = false;
  bool _configChosen = false;
  bool _disposed = false;

  Map<String, dynamic>? get created => _created;
  String? get pendingRequest => _pendingRequest;
  bool get busy => _busy;
  bool get uncertain => _uncertain;
  bool get locationEditable =>
      _admitted == null && !busy && !uncertain && !attaching;

  void selectWorktree(Map<String, dynamic> value) {
    if (!locationEditable) return;
    worktree = value;
    notifyListeners();
  }

  List<Map<String, dynamic>> get providers =>
      objects(host.snapshot['providers'])
          .where(
            (provider) =>
                provider['enabled'] == true &&
                objects(provider['models']).isNotEmpty,
          )
          .toList();

  List<Map<String, dynamic>> get worktrees => project == null
      ? []
      : objects(
          host.snapshot['worktrees'],
        ).where((tree) => tree['project'] == project!['id']).toList();

  bool get hasModel => providers.any(
    (provider) =>
        provider['id'] == config['provider'] &&
        objects(
          provider['models'],
        ).any((model) => model['id'] == config['model']),
  );

  void _changed() {
    if (busy || uncertain || attaching || created != null) return;
    if (_admitted == null) _sync();
    notifyListeners();
  }

  void _sync() {
    worktree =
        worktrees.where((tree) => tree['id'] == worktree?['id']).firstOrNull ??
        worktrees.firstOrNull;
    if (!_defaultsCaptured && !_configChosen) {
      final defaults = object(object(host.snapshot['defaults'])['config']);
      config = {
        'mode': defaults['mode'] ?? 'code',
        'permission': defaults['permission'] ?? 'ask',
        'options': defaults['options'] ?? <String, dynamic>{},
        'provider': defaults['provider'],
        'model': defaults['model'],
        'effort': defaults['effort'] ?? 'default',
        'credential': defaults['credential'],
      };
      _defaultsCaptured = host.snapshot.containsKey('defaults');
    }
    if (!hasModel && !_configChosen) {
      final provider = providers.firstOrNull;
      if (provider != null) {
        final models = objects(provider['models']);
        final model =
            models
                .where((model) => model['id'] == provider['default_model'])
                .firstOrNull ??
            models.first;
        config.addAll({
          'provider': provider['id'],
          'credential': provider['credential'],
          'model': model['id'],
          'effort': model['default_effort'] ?? 'default',
        });
      }
    }
  }

  SessionTeam get team {
    final teams = objects(host.snapshot['teams']);
    final roles = objects(host.snapshot['roles']);
    final members = _team == null
        ? roles
        : [
            for (final id in _team!['members'] as List? ?? [])
              ...roles.where((role) => role['id'] == id),
          ];
    return SessionTeam(
      {
        'config': config,
        'roles': {
          'execution': 'team',
          'team': _team,
          'profiles': members,
          'models': _models,
        },
      },
      teams,
      catalog: roles,
    );
  }

  void selectTeam(Map<String, dynamic>? value) {
    if (busy || uncertain || created != null) return;
    if (_team?['id'] == value?['id'] &&
        _team?['revision'] == value?['revision']) {
      return;
    }
    _team = value == null ? null : Map.of(value);
    _models.clear();
    _leadChosen = false;
    final lead = object(value?['model']);
    if (lead.isNotEmpty) {
      final provider = providers
          .where((provider) => provider['id'] == lead['provider'])
          .firstOrNull;
      final model = objects(
        provider?['models'],
      ).where((model) => model['id'] == lead['model']).firstOrNull;
      config = {
        ...config,
        ...lead,
        'credential': provider?['credential'],
        'effort': lead['effort'] ?? model?['default_effort'] ?? 'default',
      };
      _configChosen = true;
    }
    if (_admitted != null) _stage = _Stage.team;
    notifyListeners();
  }

  void configureMember(String role, Map<String, dynamic>? model) {
    if (busy || uncertain || created != null) return;
    if (!team.members.any((member) => member['id'] == role)) return;
    if (model == null) {
      _models.remove(role);
    } else {
      _models[role] = {
        'provider': model['provider'],
        'model': model['model'],
        'effort': model['effort'],
      };
    }
    if (_admitted != null) _stage = _Stage.team;
    notifyListeners();
  }

  void configure(Map<String, dynamic> value) {
    if (busy || uncertain || created != null) return;
    if ([
      'provider',
      'model',
      'effort',
    ].any((key) => value[key] != config[key])) {
      _leadChosen = true;
    }
    config = value;
    _configChosen = true;
    _configurationChanged = _admitted != null;
    notifyListeners();
  }

  Map<String, dynamic> _configuration() {
    if (_leadChosen) return Map.of(config);
    final admitted = object(_admitted!['config']);
    return {
      ...config,
      for (final key in ['provider', 'model', 'effort', 'credential'])
        key: admitted[key],
    };
  }

  Future<Map<String, dynamic>> _advance() => switch (_stage) {
    _Stage.create => host.command('create_session', {
      'project': project?['id'],
      'worktree': worktree?['id'],
      'config': Map<String, dynamic>.of(config),
    }),
    _Stage.team => host.command('set_session_team', {
      ...selectCommand(
        _admitted!['id'] as String,
        _admitted!['revision'] as int,
        _team,
      ),
      'models': Map<String, dynamic>.of(_models),
    }),
    _Stage.config => host.command('set_session_config', {
      'session': _admitted!['id'],
      'expected_revision': _admitted!['revision'],
      'config': _configuration(),
    }),
    _Stage.ready => throw StateError('Draft admission is complete'),
  };

  Future<void> send({required bool hasInput}) async {
    if (busy || attaching || created != null || !host.connected) return;
    if (uncertain && pendingRequest == null) return;
    if (!uncertain &&
        (project != null && worktree == null || !hasModel || !hasInput)) {
      return;
    }
    _busy = true;
    notifyListeners();
    try {
      while (_stage != _Stage.ready && !_disposed) {
        final result = uncertain
            ? await host.execute(pendingRequest!)
            : await _advance();
        if (_disposed) return;
        _admitted = object(result['data']);
        _pendingRequest = null;
        _uncertain = false;
        _stage = switch (_stage) {
          _Stage.create when _team != null || _models.isNotEmpty => _Stage.team,
          _Stage.team when _leadChosen || _configurationChanged =>
            _Stage.config,
          _ => _Stage.ready,
        };
      }
      if (!_disposed) _created = _admitted;
    } catch (error) {
      if (!_disposed) {
        if (error is CommandFailure && error.code == 'outcome_unknown') {
          _uncertain = true;
          _pendingRequest = error.request ?? pendingRequest;
        } else if (error is CommandFailure && error.rejected) {
          _uncertain = false;
          _pendingRequest = null;
        }
      }
      rethrow;
    } finally {
      if (!_disposed) {
        _busy = false;
        notifyListeners();
      }
    }
  }

  @override
  void dispose() {
    _disposed = true;
    host.removeListener(_changed);
    super.dispose();
  }
}
