import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart' show TextEditingController;

import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import 'attachments.dart';
import 'team.dart';

/// Captures a new session's resources until Node admission is resolved.
class CreationDraft extends ChangeNotifier {
  CreationDraft(this.host, {String? initialProject, String? initialWorktree})
    : _projectId = initialProject == '' ? null : initialProject {
    worktree = worktrees
        .where((tree) => tree['id'] == initialWorktree)
        .firstOrNull;
    _sync();
    host.addListener(_changed);
  }

  static final _drafts = Expando<CreationDraft>();

  static CreationDraft retain(
    HostConnection host, {
    String? initialProject,
    String? initialWorktree,
  }) {
    final draft = _drafts[host] ??= CreationDraft(
      host,
      initialProject: initialProject,
      initialWorktree: initialWorktree,
    );
    draft._retained = true;
    draft._views++;
    return draft;
  }

  final HostConnection host;
  final input = TextEditingController();
  final _attachments = <String, List<PickedAttachment>>{};

  List<PickedAttachment> get files =>
      _attachments.putIfAbsent(text(worktree?['id']), () => []);

  void clearInput() {
    input.clear();
    _attachments.clear();
  }

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
  ({String id, int revision})? _admitted;
  Map<String, dynamic>? _team;
  final _models = <String, dynamic>{};
  bool _leadChosen = false;
  String? _pendingRequest;
  bool _busy = false;
  bool _unavailable = false;
  bool _missingReceipt = false;
  bool _attaching = false;
  bool _defaultsCaptured = false;
  bool _configChosen = false;
  bool _disposed = false;
  bool _retained = false;
  bool _presented = false;
  int _views = 0;

  Map<String, dynamic>? get created => _created;
  String? get pendingRequest => _pendingRequest;
  bool get busy => _busy;
  bool get unavailable => _unavailable;
  bool get canCheck =>
      pendingRequest != null ||
      (_admitted != null && created == null && !unavailable);
  bool get uncertain => canCheck || _missingReceipt;
  bool get locationEditable =>
      _admitted == null && !busy && !uncertain && !attaching;

  bool get attaching => _attaching;
  set attaching(bool value) {
    if (_disposed || _attaching == value) return;
    _attaching = value;
    notifyListeners();
  }

  void selectWorktree(Map<String, dynamic> value) {
    if (!locationEditable) return;
    worktree = value;
    notifyListeners();
  }

  List<Map<String, dynamic>> get providers => host.executableProviders
      .where((provider) => objects(provider['models']).isNotEmpty)
      .toList();

  List<Map<String, dynamic>> get worktrees => project == null
      ? []
      : objects(
          host.snapshot['worktrees'],
        ).where((tree) => tree['project'] == project!['id']).toList();

  bool get hasModel =>
      host.ownsCredential(config['credential']) &&
      providers.any(
        (provider) =>
            provider['id'] == config['provider'] &&
            objects(
              provider['models'],
            ).any((model) => model['id'] == config['model']),
      );

  void _changed() {
    if (busy || attaching || created != null || unavailable) return;
    if (uncertain) {
      _observeAdmission();
      if (_admitted case final admitted?) {
        _created = _observedSession(admitted);
      }
      if (created == null) return;
    } else {
      _sync();
    }
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
        if (defaults['features'] != null) 'features': defaults['features'],
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
        config.remove('features');
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
      providers: providers,
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
        if (model.containsKey('features')) 'features': model['features'],
      };
    }
    notifyListeners();
  }

  void configure(Map<String, dynamic> value) {
    if (busy || uncertain || created != null) return;
    if ([
          'provider',
          'model',
          'effort',
        ].any((key) => value[key] != config[key]) ||
        !mapEquals(object(value['features']), object(config['features']))) {
      _leadChosen = true;
    }
    config = value;
    _configChosen = true;
    notifyListeners();
  }

  String _requestId() {
    final request = pendingRequest;
    if (request == null) return '';
    try {
      return text(object(jsonDecode(request))['id']);
    } on FormatException {
      return '';
    }
  }

  void _observeAdmission() {
    final id = _requestId();
    if (id.isEmpty) return;
    for (final turn in objects(host.snapshot['turns'])) {
      final session = text(turn['session']);
      if (turn['request'] == id &&
          session.isNotEmpty &&
          text(turn['id']).isNotEmpty) {
        _admitted = (id: session, revision: 0);
        _pendingRequest = null;
        return;
      }
    }
  }

  Map<String, dynamic>? _observedSession(
    ({String id, int revision}) admitted,
  ) => objects(host.snapshot['sessions']).where((session) {
    return session['id'] == admitted.id &&
        number(session['revision']) >= admitted.revision;
  }).firstOrNull;

  Future<void> send({
    required String message,
    List<String> attachments = const [],
  }) => _settle(message: message, attachments: attachments);

  Future<void> check() => _settle(checkOnly: true);

  Future<void> _settle({
    String message = '',
    List<String> attachments = const [],
    bool checkOnly = false,
  }) async {
    if (_disposed ||
        busy ||
        attaching ||
        created != null ||
        unavailable ||
        !host.connected ||
        (checkOnly && !canCheck) ||
        (_missingReceipt && !canCheck)) {
      return;
    }
    if (!uncertain &&
        (project != null && worktree == null ||
            !hasModel ||
            message.trim().isEmpty && attachments.isEmpty)) {
      return;
    }
    _busy = true;
    notifyListeners();
    try {
      _observeAdmission();
      if (_admitted == null) {
        final request = pendingRequest;
        try {
          final result = request != null
              ? await host.check(request)
              : await host.command('start_session', {
                  'project': project?['id'],
                  'worktree': worktree?['id'],
                  'config': Map.of(config),
                  'message': {
                    'text': message,
                    'attachments': List.of(attachments),
                  },
                  'execution': 'team',
                  'team': teamSelection(_team),
                  'models': Map.of(_models),
                  if (_leadChosen) 'use_explicit_config': true,
                }, (prepared) => _pendingRequest = prepared);
          if (_disposed) return;
          final turn = object(result['data']);
          final id = text(turn['session']);
          final revision = turn['revision'];
          final requestId = _requestId();
          if (result['kind'] != 'queued_turn' ||
              id.isEmpty ||
              text(turn['id']).isEmpty ||
              revision is! int ||
              revision < 1 ||
              (requestId.isNotEmpty && turn['request'] != requestId)) {
            _missingReceipt = pendingRequest == null;
            throw CommandFailure('outcome_unknown', request: pendingRequest);
          }
          _admitted = (id: id, revision: revision);
          _pendingRequest = null;
        } on CommandFailure catch (error) {
          if (_disposed) rethrow;
          if (error.retained) _pendingRequest = error.request;
          _observeAdmission();
          if (_admitted == null) {
            if (error.rejected) _pendingRequest = null;
            rethrow;
          }
        }
      }

      // Admission is durable. A missing session read must not submit again.
      final admitted = _admitted!;
      final observed = _observedSession(admitted);
      if (observed != null) {
        _created = observed;
        return;
      }
      final Map<String, dynamic> result;
      try {
        result = await host.command('read_session', {'session': admitted.id});
      } on CommandFailure catch (error) {
        if (!const {'not_found', 'permission_denied'}.contains(error.code)) {
          rethrow;
        }
        if (!_disposed) _unavailable = true;
        return;
      }
      if (_disposed) return;
      final session = object(result['data']);
      if (result['kind'] != 'session' ||
          session['id'] != admitted.id ||
          number(session['revision']) < admitted.revision) {
        throw const CommandFailure('outcome_unknown');
      }
      _created = session;
    } finally {
      if (!_disposed) {
        _busy = false;
        notifyListeners();
        _releaseIfSettled();
      }
    }
  }

  void presented() {
    if (created == null) return;
    _presented = true;
    if (identical(_drafts[host], this)) _drafts[host] = null;
  }

  void release() {
    _views--;
    _releaseIfSettled();
  }

  void _releaseIfSettled() {
    if (!_retained ||
        _views != 0 ||
        busy ||
        uncertain ||
        (created != null && !_presented)) {
      return;
    }
    dispose();
  }

  @override
  void dispose() {
    _disposed = true;
    if (identical(_drafts[host], this)) _drafts[host] = null;
    host.removeListener(_changed);
    input.dispose();
    super.dispose();
  }
}
