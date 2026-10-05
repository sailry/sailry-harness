import '../../content/diff.dart';
export '../../content/diff.dart' show DiffView;
import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../runtime/notices.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import 'workspace.dart';

class LiveGitPage extends StatefulWidget {
  const LiveGitPage({super.key, this.hostId, this.worktreeId});
  final String? hostId;
  final String? worktreeId;
  @override
  State<LiveGitPage> createState() => _LiveGitPageState();
}

class _LiveGitPageState extends State<LiveGitPage> {
  late String? _hostId = widget.hostId;
  ResourceTarget? _target;
  String? _identity;
  Map<String, dynamic> _status = {};
  Map<String, dynamic> _branches = {};
  Map<String, dynamic> _log = {};
  String _tab = 'changes';
  String? _error;
  String? _pending;
  bool _busy = false;
  int _generation = 0;
  bool get _canWrite =>
      !_busy && _pending == null && _status['index_revision'] != null;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final target = resourceTarget(context, _hostId, widget.worktreeId);
    final identity = target == null
        ? null
        : '${target.host.id}/${target.worktree['id']}';
    _target = target;
    if (target != null) _hostId ??= target.host.id;
    if (_identity != identity) {
      _identity = identity;
      _status = {};
      _branches = {};
      _log = {};
      _generation++;
      if (target != null) _load();
    }
  }

  Map<String, dynamic> get _guard => {
    'expected_index': _status['index_revision'],
    'expected_head': _status['head'],
    'expected_branch': _status['branch'],
  };

  Future<void> _load({bool refresh = true}) async {
    final target = _target;
    if (target == null) return;
    final generation = ++_generation;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final data = {'worktree': target.worktree['id']};
      if (refresh || _status.isEmpty) {
        final status = await target.host.command('inspect_git', data);
        if (!mounted || generation != _generation) return;
        _status = object(status['data']);
        _branches = {};
        _log = {};
      }
      if (_status['kind'] == 'directory') return;
      if (_tab == 'branches' && _branches.isEmpty) {
        final branches = await target.host.command('list_git_branches', data);
        if (!mounted || generation != _generation) return;
        _branches = object(branches['data']);
      } else if (_tab == 'history' && _log.isEmpty) {
        final log = await target.host.command('read_git_log', {
          ...data,
          'limit': 100,
          'cursor': null,
        });
        if (!mounted || generation != _generation) return;
        _log = object(log['data']);
      }
    } catch (error) {
      if (mounted && generation == _generation) {
        setState(() => _error = failureText(error, translate: context.tr));
      }
    } finally {
      if (mounted && generation == _generation) setState(() => _busy = false);
    }
  }

  void _selectTab(String tab) {
    if (_busy || _tab == tab) return;
    setState(() => _tab = tab);
    if (tab == 'branches' && _branches.isEmpty ||
        tab == 'history' && _log.isEmpty) {
      _load(refresh: false);
    }
  }

  Future<void> _mutate(String kind, Map<String, dynamic> data) async {
    if (!_canWrite) return;
    final target = _target!;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final result = await target.host.command(kind, {
        if (kind != 'create_managed_worktree')
          'worktree': target.worktree['id'],
        ...data,
      });
      if (!mounted) return;
      final followUp = object(result['data'])['follow_up'];
      await _load();
      if (mounted && followUp != null) {
        setState(() => _error = text(object(followUp)['message']));
      }
    } on CommandFailure catch (error) {
      if (mounted) {
        setState(() {
          _pending = error.code == 'outcome_unknown' ? error.request : null;
          _error = failureText(error, translate: context.tr);
        });
      }
    } catch (error) {
      if (mounted) {
        setState(() => _error = failureText(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _retry() async {
    final pending = _pending;
    final target = _target;
    if (pending == null || target == null) return;
    setState(() => _busy = true);
    try {
      await target.host.execute(pending);
      if (!mounted) return;
      setState(() => _pending = null);
      await _load();
    } on CommandFailure catch (error) {
      if (mounted) {
        setState(() {
          if (error.code != 'outcome_unknown') _pending = null;
          _error = failureText(error, translate: context.tr);
        });
      }
    } catch (error) {
      if (mounted) {
        setState(() => _error = failureText(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _diff(Map<String, dynamic> entry, bool staged) async {
    final target = _target;
    if (target == null) return;
    final identity = _identity;
    final index = _status['index_revision'];
    final head = _status['head'];
    try {
      final path = text(entry['path']);
      final result = await target.host.command('read_git_diff', {
        'worktree': target.worktree['id'],
        'path': path,
        'scope': staged ? 'staged' : 'unstaged',
      });
      if (!mounted || identity != _identity) return;
      final diff = object(result['data']);
      final apply = await showAppSheet<bool>(
        context,
        path,
        child: Builder(
          builder: (context) => Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              DiffView(diff: diff),
              const SizedBox(height: 16),
              OutlinedButton(
                onPressed: _canWrite
                    ? () => Navigator.pop(context, true)
                    : null,
                child: Text(context.tr(staged ? 'unstage' : 'stage')),
              ),
            ],
          ),
        ),
      );
      if (mounted && identity == _identity && apply == true) {
        await _mutate('update_git_index', {
          'paths': [path],
          'operation': staged ? 'unstage' : 'stage',
          'expected_index': index,
          'expected_head': head,
        });
      }
    } catch (error) {
      if (mounted) {
        setState(() => _error = failureText(error, translate: context.tr));
      }
    }
  }

  Future<void> _commit() async {
    final message = await askResourceText(
      context,
      'commitMessage',
      multiline: true,
    );
    if (!mounted || message == null) return;
    await _mutate('create_git_commit', {
      ..._guard,
      'message': message,
      'amend': false,
      'options': {
        'signoff': false,
        'skip_hooks': false,
        'tracked': false,
        'all': false,
        'after': 'none',
        'push_remote': null,
      },
    });
  }

  Future<void> _actions() async {
    final action = await showAppSheet<String>(
      context,
      context.tr('gitActions'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final item in [
              ('fetch', 'gitFetch'),
              ('pull', 'gitPull'),
              ('push', 'gitPush'),
            ])
              ListTile(
                title: Text(context.tr(item.$2)),
                onTap: () => Navigator.pop(context, item.$1),
              ),
          ],
        ),
      ),
    );
    if (!mounted || action == null) return;
    await _mutate('run_git_action', {
      ..._guard,
      'action': switch (action) {
        'fetch' => {
          'fetch': {'remote': null, 'prune': false, 'all': false},
        },
        'pull' => {
          'pull': {'rebase': false, 'remote': null, 'branch': null},
        },
        _ => {
          'push': {'remote': null, 'publish': false, 'force': false},
        },
      },
    });
  }

  Future<void> _branch(Map<String, dynamic> branch) async {
    if (branch['current'] == true || branch['remote'] == true) return;
    final name = text(branch['name']);
    final action = await showAppSheet<String>(
      context,
      name,
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final item in [
              ('switch', 'gitSwitch'),
              ('merge', 'gitMerge'),
              ('delete', 'gitDeleteBranch'),
            ])
              ListTile(
                title: Text(context.tr(item.$2)),
                onTap: () => Navigator.pop(context, item.$1),
              ),
          ],
        ),
      ),
    );
    if (!mounted || action == null) return;
    final confirmed = await showAppDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(
          context.tr(
            {
              'switch': 'gitSwitch',
              'merge': 'gitMerge',
              'delete': 'gitDeleteBranch',
            }[action]!,
          ),
        ),
        content: Text(name),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(context.tr('cancel')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(context.tr('confirm')),
          ),
        ],
      ),
    );
    if (!mounted || confirmed != true) return;
    await _mutate('${action}_git_branch', {
      'name': name,
      'commit': branch['commit'],
      if (action != 'delete') 'expected_index': _status['index_revision'],
      'expected_head': _status['head'],
      'expected_branch': _status['branch'],
    });
  }

  Future<void> _createBranch({bool worktree = false}) async {
    final branch = await askResourceText(context, 'gitBranchName');
    if (!mounted || branch == null) return;
    await _mutate(
      worktree ? 'create_managed_worktree' : 'create_git_branch',
      worktree
          ? {
              'project': _target!.worktree['project'],
              'source': _target!.worktree['id'],
              'branch': branch,
              'expected_head': _status['head'],
              'expected_index': _status['index_revision'],
              'include_changes': false,
            }
          : {'name': branch, 'commit': _status['head']},
    );
  }

  Future<void> _history(Map<String, dynamic> entry) async {
    try {
      final result = await _target!.host.command('read_git_commit', {
        'worktree': _target!.worktree['id'],
        'commit': entry['id'],
      });
      if (!mounted) return;
      final commit = object(result['data']);
      await showAppSheet<void>(
        context,
        text(entry['message']),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final diff in objects(commit['files']))
              Padding(
                padding: const EdgeInsets.only(bottom: 12),
                child: DiffView(diff: diff),
              ),
            if (commit['truncated'] == true)
              Text(context.tr('resourcePartial')),
          ],
        ),
      );
    } catch (error) {
      if (mounted) {
        setState(() => _error = failureText(error, translate: context.tr));
      }
    }
  }

  Future<void> _more() async {
    if (_log['next'] == null || _target == null) return;
    setState(() => _busy = true);
    try {
      final result = await _target!.host.command('read_git_log', {
        'worktree': _target!.worktree['id'],
        'limit': 100,
        'cursor': _log['next'],
      });
      if (!mounted) return;
      final log = object(result['data']);
      setState(() {
        log['entries'] = [
          ...objects(_log['entries']),
          ...objects(log['entries']),
        ];
        _log = log;
      });
    } catch (error) {
      if (mounted) {
        setState(() => _error = failureText(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final entries = objects(_status['entries']);
    final colors = Theme.of(context).colorScheme;
    final added = entries.fold<num>(
      0,
      (sum, entry) => sum + number(object(entry['diff'])['additions']),
    );
    final removed = entries.fold<num>(
      0,
      (sum, entry) => sum + number(object(entry['diff'])['deletions']),
    );
    final empty = _target == null
        ? 'resourceNoWorkspace'
        : !_target!.host.connected
        ? 'hostDisconnected'
        : _busy || _error != null || _pending != null
        ? null
        : _status['kind'] == 'directory'
        ? 'resourceGitNotRepository'
        : _tab == 'changes' && entries.isEmpty
        ? 'noChanges'
        : _tab == 'branches' && objects(_branches['entries']).isEmpty ||
              _tab == 'history' && objects(_log['entries']).isEmpty
        ? 'resourceEmpty'
        : null;
    return PageFrame(
      loading: _busy,
      title: context.tr('git'),
      failure: _target != null && !_target!.host.connected
          ? const HostState(added: true)
          : _error != null
          ? FailureState(
              icon: 'branch',
              message: _error!,
              onRetry: _busy
                  ? null
                  : _pending == null
                  ? _load
                  : _retry,
            )
          : null,
      empty: empty == null
          ? null
          : EmptyState(icon: 'branch', message: context.tr(empty)),
      actions: [
        RoundButton(
          icon: 'refresh',
          tooltip: context.tr('refresh'),
          onPressed: _busy ? null : _load,
        ),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (_target != null && _target!.host.connected) ...[
            if (_pending != null)
              FilledButton(
                onPressed: _busy ? null : _retry,
                child: Text(context.tr('retry')),
              ),
            if (_status['kind'] != 'directory') ...[
              Surface(
                radius: 16,
                onTap: _canWrite ? _actions : null,
                child: Row(
                  children: [
                    const AppIcon('branch'),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Text(
                        text(_status['branch'], text(_status['head'])),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                    Text('+$added', style: TextStyle(color: colors.tertiary)),
                    const SizedBox(width: 8),
                    Text(
                      '−$removed',
                      style: TextStyle(color: colors.secondary),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 16),
              SegmentedButton<String>(
                showSelectedIcon: false,
                segments: [
                  for (final item in [
                    ('changes', 'changes'),
                    ('branches', 'gitBranches'),
                    ('history', 'gitHistory'),
                  ])
                    ButtonSegment(
                      value: item.$1,
                      label: Text(context.tr(item.$2)),
                    ),
                ],
                selected: {_tab},
                onSelectionChanged: (value) => _selectTab(value.single),
                style: SegmentedButton.styleFrom(
                  side: BorderSide.none,
                  selectedBackgroundColor: colors.surfaceContainerHigh,
                  backgroundColor: Colors.transparent,
                ),
              ),
              const SizedBox(height: 16),
              if (_tab == 'changes' && entries.isNotEmpty) ...[
                for (final staged in [false, true])
                  ExpansionTile(
                    key: ValueKey('live-git-$staged'),
                    initiallyExpanded: true,
                    tilePadding: EdgeInsets.zero,
                    title: Text(context.tr(staged ? 'staged' : 'workingTree')),
                    children: [
                      for (final entry in entries.where(
                        (entry) => staged
                            ? entry['staged'] != null
                            : entry['unstaged'] != null ||
                                  entry['untracked'] == true ||
                                  entry['conflicted'] == true,
                      ))
                        ListTile(
                          leading: const AppIcon('file'),
                          title: Text(text(entry['path'])),
                          trailing: const AppIcon('chevron'),
                          onTap: _busy ? null : () => _diff(entry, staged),
                        ),
                    ],
                  ),
                FilledButton(
                  onPressed:
                      _canWrite &&
                          entries.any((entry) => entry['staged'] != null)
                      ? _commit
                      : null,
                  child: Text(context.tr('commit')),
                ),
              ],
              if (_tab == 'branches') ...[
                if (objects(_branches['entries']).isNotEmpty)
                  Surface(
                    padding: const EdgeInsets.symmetric(horizontal: 12),
                    child: Column(
                      children: [
                        for (final branch in objects(_branches['entries']))
                          ListTile(
                            leading: const AppIcon('branch'),
                            title: Text(text(branch['name'])),
                            subtitle: branch['current'] == true
                                ? Text(context.tr('gitCurrent'))
                                : null,
                            trailing: AppIcon(
                              branch['current'] == true ? 'check' : 'chevron',
                            ),
                            onTap: _canWrite ? () => _branch(branch) : null,
                          ),
                      ],
                    ),
                  ),
                OutlinedButton.icon(
                  onPressed: _canWrite && _status['head'] != null
                      ? _createBranch
                      : null,
                  icon: const AppIcon('plus'),
                  label: Text(context.tr('gitCreateBranch')),
                ),
                OutlinedButton.icon(
                  onPressed:
                      _canWrite &&
                          _status['head'] != null &&
                          _target!.worktree['project'] != null
                      ? () => _createBranch(worktree: true)
                      : null,
                  icon: const AppIcon('branch'),
                  label: Text(context.tr('resourceWorktreeCreate')),
                ),
                if (_branches['truncated'] == true)
                  Text(context.tr('resourcePartial')),
              ],
              if (_tab == 'history') ...[
                if (objects(_log['entries']).isNotEmpty)
                  Surface(
                    padding: const EdgeInsets.symmetric(horizontal: 12),
                    child: Column(
                      children: [
                        for (final entry in objects(_log['entries']))
                          ListTile(
                            leading: const AppIcon('clock'),
                            title: Text(text(entry['message'])),
                            subtitle: Text(text(entry['id'])),
                            trailing: const AppIcon('chevron'),
                            onTap: _busy ? null : () => _history(entry),
                          ),
                      ],
                    ),
                  ),
                if (_log['next'] != null)
                  TextButton(
                    onPressed: _busy ? null : _more,
                    child: Text(context.tr('resourceMore')),
                  ),
              ],
            ],
          ],
        ],
      ),
    );
  }
}
