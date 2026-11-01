import '../../content/diff.dart';
export '../../content/diff.dart' show DiffView;
import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../runtime/json.dart';
import '../../runtime/notices.dart';
import '../../runtime/session.dart';
import '../../ui/kit.dart';
import '../../ui/pill_tabs.dart';
import '../../ui/toast.dart';
import 'git/branch_card.dart';
import 'git/branch_list.dart';
import 'git/change_row.dart';
import 'git/commit_bar.dart';
import 'git/diff_sheet.dart';
import 'git/history_row.dart';
import 'git/model.dart';
import 'workspace.dart';

class GitPage extends StatefulWidget {
  const GitPage({super.key, this.hostId, this.worktreeId});
  final String? hostId;
  final String? worktreeId;
  @override
  State<GitPage> createState() => _GitPageState();
}

class _GitPageState extends State<GitPage> {
  late String? _hostId = widget.hostId;
  final _message = TextEditingController();
  CommitChoice _choice = (skipHooks: false, push: false);
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

  @override
  void dispose() {
    _message.dispose();
    super.dispose();
  }

  Map<String, dynamic> get _guard => {
    'expected_index': _status['index_revision'],
    'expected_head': _status['head'],
    'expected_branch': _status['branch'],
  };

  void _toast(Object error) {
    if (mounted) showFailure(context, error);
  }

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
      // The branch card needs upstream sync on every tab.
      if (_branches.isEmpty) {
        final branches = await target.host.command('list_git_branches', data);
        if (!mounted || generation != _generation) return;
        _branches = object(branches['data']);
      }
      if (_tab == 'history' && _log.isEmpty) {
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

  /// Returns whether the Node accepted the mutation.
  Future<bool> _mutate(String kind, Map<String, dynamic> data) async {
    if (!_canWrite) return false;
    final target = _target!;
    setState(() => _busy = true);
    try {
      final result = await target.host.command(kind, {
        if (kind != 'create_managed_worktree')
          'worktree': target.worktree['id'],
        ...data,
      });
      if (!mounted) return true;
      final followUp = object(result['data'])['follow_up'];
      await _load();
      if (mounted && followUp != null) {
        showToast(context, text(object(followUp)['message']));
      }
      return true;
    } on CommandFailure catch (error) {
      if (mounted) {
        setState(
          () =>
              _pending = error.code == 'outcome_unknown' ? error.request : null,
        );
      }
      _toast(error);
    } catch (error) {
      _toast(error);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
    return false;
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
      if (mounted && error.code != 'outcome_unknown') {
        setState(() => _pending = null);
      }
      _toast(error);
    } catch (error) {
      _toast(error);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<bool> _confirm(String title, String message) async =>
      await showAppDialog<bool>(
        context: context,
        builder: (context) => AlertDialog(
          title: Text(title),
          content: Text(message),
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
      ) ==
      true;

  Future<void> _diff(Entry entry, bool staged) async {
    final target = _target;
    if (target == null) return;
    final identity = _identity;
    // Index actions apply to the revision the user reviewed.
    final guard = _guard;
    try {
      final path = text(entry['path']);
      final result = await target.host.command('read_git_diff', {
        'worktree': target.worktree['id'],
        'path': path,
        'scope': staged ? 'staged' : 'unstaged',
      });
      if (!mounted || identity != _identity) return;
      final action = await showDiffSheet(
        context,
        entry: entry,
        staged: staged,
        diff: object(result['data']),
        enabled: _canWrite,
      );
      if (!mounted || identity != _identity || action == null) return;
      if (action == DiffAction.toggle) {
        await _mutate('update_git_index', {
          'paths': [path],
          'operation': staged ? 'unstage' : 'stage',
          'expected_index': guard['expected_index'],
          'expected_head': guard['expected_head'],
        });
        return;
      }
      final trash = discardKind(entry) == DiscardKind.trash;
      final name = path.split('/').last;
      if (!await _confirm(
        context.tr(trash ? 'fileTrash' : 'discard'),
        context
            .tr(trash ? 'gitTrashConfirm' : 'gitDiscardConfirm')
            .replaceAll('{name}', name),
      )) {
        return;
      }
      if (!mounted || identity != _identity) return;
      await _mutate('run_git_action', {
        ...guard,
        'action': {
          trash ? 'trash' : 'discard': {
            'paths': [path],
          },
        },
      });
    } catch (error) {
      _toast(error);
    }
  }

  Future<void> _commit() async {
    final message = _message.text.trim();
    if (message.isEmpty) return;
    final committed = await _mutate('create_git_commit', {
      ..._guard,
      'message': message,
      'amend': false,
      'options': commitOptions(
        skipHooks: _choice.skipHooks,
        push: _choice.push,
      ),
    });
    if (committed && mounted) _message.clear();
  }

  Future<void> _action(String action) => _mutate('run_git_action', {
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

  Future<void> _branch(Entry branch) async {
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
    final confirmed = await _confirm(
      context.tr(
        {
          'switch': 'gitSwitch',
          'merge': 'gitMerge',
          'delete': 'gitDeleteBranch',
        }[action]!,
      ),
      name,
    );
    if (!mounted || !confirmed) return;
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

  Future<void> _history(Entry entry) async {
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
      _toast(error);
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
      _toast(error);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final target = _target;
    final connected = target != null && target.host.connected;
    final entries = objects(_status['entries']);
    final groups = groupChanges(entries);
    final sync = object(_branches['sync']);
    final branchSync = _branches.isEmpty
        ? null
        : (
            upstream: sync['upstream'] as String?,
            ahead: number(sync['ahead']).toInt(),
            behind: number(sync['behind']).toInt(),
          );
    final repository =
        connected && _status.isNotEmpty && _status['kind'] != 'directory';
    final empty = target == null
        ? 'resourceNoWorkspace'
        : !target.host.connected
        ? 'hostDisconnected'
        : _busy || _error != null
        ? null
        : _status['kind'] == 'directory'
        ? 'resourceGitNotRepository'
        : _tab == 'changes' && entries.isEmpty
        ? 'noChanges'
        : _tab == 'history' && objects(_log['entries']).isEmpty
        ? 'resourceEmpty'
        : null;
    return PageFrame(
      loading: _busy,
      title: context.tr('git'),
      failure: target != null && !target.host.connected
          ? const HostState(added: true)
          : _error != null
          ? FailureState(
              icon: 'branch',
              message: _error!,
              onRetry: _busy ? null : _load,
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
      bottom: repository && _tab == 'changes' && groups.staged.isNotEmpty
          ? CommitBar(
              controller: _message,
              count: groups.staged.length,
              choice: _choice,
              onChoice: (choice) => setState(() => _choice = choice),
              onCommit: _canWrite ? _commit : null,
            )
          : null,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (connected && _pending != null) ...[
            Text(context.tr('failureUnknown')),
            const SizedBox(height: 8),
            FilledButton(
              onPressed: _busy ? null : _retry,
              child: Text(context.tr('retry')),
            ),
            const SizedBox(height: 16),
          ],
          if (repository) ...[
            BranchCard(
              branch: text(_status['branch'], shortHash(text(_status['head']))),
              sync: branchSync,
              onBranch: _busy ? null : () => _selectTab('branches'),
              onAction: _canWrite ? _action : null,
            ),
            const SizedBox(height: 14),
            PillTabs<String>(
              expand: true,
              tabs: [
                (
                  value: 'changes',
                  label: context.tr('changes'),
                  count: entries.length,
                ),
                (
                  value: 'branches',
                  label: context.tr('gitBranches'),
                  count: null,
                ),
                (
                  value: 'history',
                  label: context.tr('gitHistory'),
                  count: null,
                ),
              ],
              selected: _tab,
              onSelected: _busy ? null : _selectTab,
            ),
            const SizedBox(height: 6),
            if (_tab == 'changes')
              for (final (staged, items) in [
                (false, groups.unstaged),
                (true, groups.staged),
              ])
                if (items.isNotEmpty) ...[
                  _GroupTitle(
                    context.tr(staged ? 'staged' : 'gitUnstaged'),
                    items.length,
                  ),
                  for (final (index, entry) in items.indexed)
                    ChangeRow(
                      entry: entry,
                      staged: staged,
                      divider: index < items.length - 1,
                      onTap: _busy ? null : () => _diff(entry, staged),
                    ),
                ],
            if (_tab == 'branches' && _branches.isNotEmpty)
              BranchList(
                branches: objects(_branches['entries']),
                truncated: _branches['truncated'] == true,
                onBranch: _canWrite ? _branch : null,
                onCreate: _canWrite && _status['head'] != null
                    ? _createBranch
                    : null,
                onWorktree:
                    _canWrite &&
                        _status['head'] != null &&
                        target.worktree['project'] != null
                    ? () => _createBranch(worktree: true)
                    : null,
              ),
            if (_tab == 'history') ...[
              for (final (index, entry) in objects(_log['entries']).indexed)
                HistoryRow(
                  entry: entry,
                  branch: _status['branch'] as String?,
                  now: DateTime.now(),
                  divider: index < objects(_log['entries']).length - 1,
                  onTap: _busy ? null : () => _history(entry),
                ),
              if (_log['next'] != null)
                TextButton(
                  onPressed: _busy ? null : _more,
                  child: Text(context.tr('resourceMore')),
                ),
            ],
          ],
        ],
      ),
    );
  }
}

class _GroupTitle extends StatelessWidget {
  const _GroupTitle(this.label, this.count);
  final String label;
  final int count;

  @override
  Widget build(BuildContext context) {
    final dim = Theme.of(
      context,
    ).colorScheme.onSurfaceVariant.withValues(alpha: .65);
    return Padding(
      padding: const EdgeInsets.only(top: 14, bottom: 2),
      child: Text('$label  $count', style: TextStyle(fontSize: 13, color: dim)),
    );
  }
}
