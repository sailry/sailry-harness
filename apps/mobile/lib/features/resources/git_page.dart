import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../runtime/session.dart';
import 'live_git.dart';
import 'files_page.dart';
import 'git_fixtures.dart';
import 'workspace.dart';

class GitPage extends StatefulWidget {
  const GitPage({
    super.key,
    this.hostId,
    this.worktreeId,
    this.host = 'Studio',
    this.project = 'sailry-web',
    this.branch = 'feature/sign-in',
  });
  final String host;
  final String? hostId;
  final String? worktreeId;
  final String project;
  final String branch;

  @override
  State<GitPage> createState() => _GitPageState();
}

class _GitPageState extends State<GitPage> {
  String get _project => widget.project;
  String get _worktree => widget.branch;
  final Map<String, GitPreview> _previews = {};
  String _tab = 'changes';

  GitPreview get _git => _previews.putIfAbsent(
    '${widget.host}/$_project/$_worktree',
    () => GitPreview(_worktree, translate: context.tr),
  );

  Future<void> _actions() async {
    final action = await showAppSheet<String>(
      context,
      context.tr('gitActions'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final item in [
              ('refresh', 'gitFetch', 'fetch', ''),
              ('refresh', 'gitPull', 'pull', '${_git.behind}'),
              ('send', 'gitPush', 'push', '${_git.ahead}'),
            ])
              ListTile(
                contentPadding: EdgeInsets.zero,
                leading: AppIcon(item.$1),
                title: Text(context.tr(item.$2)),
                trailing: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(item.$4),
                    const SizedBox(width: 8),
                    const AppIcon('chevron'),
                  ],
                ),
                onTap: () => Navigator.pop(context, item.$3),
              ),
          ],
        ),
      ),
    );
    if (action == null || !mounted) return;
    if (action == 'pull' && !_git.committed) {
      showResourceNotice(context, 'gitDirty');
      return;
    }
    setState(() {
      if (action == 'pull') _git.behind = 0;
      if (action == 'push') _git.ahead = 0;
    });
    showResourceNotice(context, 'gitPreview');
  }

  Future<void> _selectDiff() async {
    final name = await showAppSheet<String>(
      context,
      context.tr('diffSelection'),
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final file in gitDiffs.keys)
              ListTile(
                contentPadding: EdgeInsets.zero,
                leading: const AppIcon('file'),
                title: Text(file),
                trailing: AppIcon(_git.file == file ? 'check' : 'chevron'),
                onTap: () => Navigator.pop(context, file),
              ),
          ],
        ),
      ),
    );
    if (name != null && mounted) setState(() => _git.file = name);
  }

  Future<void> _commit() async {
    final message = await showAppSheet<String>(
      context,
      context.tr('commitTitle'),
      child: _GitForm(
        label: 'commitMessage',
        hint: 'commitPlaceholder',
        action: 'commitPreview',
        multiline: true,
      ),
    );
    if (message == null || !mounted) return;
    setState(() {
      _git.history.insert(0, (
        id: 'sample-${_git.sequence++}',
        title: message,
        branch: _git.branch,
      ));
      _git.committed = true;
      _git.staged = false;
      _git.ahead++;
    });
    showResourceNotice(context, 'committed');
  }

  Future<void> _createBranch() async {
    final name = await showAppSheet<String>(
      context,
      context.tr('gitCreateBranch'),
      child: _GitForm(
        label: 'gitBranchName',
        action: 'create',
        validate: (value) {
          if (!RegExp(r'^[a-zA-Z0-9][a-zA-Z0-9._/-]*$').hasMatch(value) ||
              RegExp(r'\.\.|//|/$|\.$|\.lock$').hasMatch(value) ||
              _git.branches.contains(value)) {
            return context.tr('gitInvalidBranch');
          }
          return null;
        },
      ),
    );
    if (name != null && mounted) setState(() => _git.branches.add(name));
  }

  Future<void> _branchActions(String name) async {
    final action = await showAppSheet<String>(
      context,
      name,
      child: Builder(
        builder: (context) => Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (name == _git.branch)
              Text(context.tr('gitCurrent'))
            else
              for (final item in [
                ('branch', 'gitSwitch', 'switch'),
                ('branch', 'gitMerge', 'merge'),
                ('trash', 'gitDeleteBranch', 'delete'),
              ])
                ListTile(
                  leading: AppIcon(item.$1),
                  title: Text(context.tr(item.$2)),
                  trailing: const AppIcon('chevron'),
                  onTap: () => Navigator.pop(context, item.$3),
                ),
          ],
        ),
      ),
    );
    if (action == null || !mounted) return;
    if (action != 'delete' && !_git.committed) {
      showResourceNotice(context, 'gitDirty');
      return;
    }
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
        content: Text(action == 'merge' ? '$name → ${_git.branch}' : name),
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
    if (confirmed != true || !mounted) return;
    setState(() {
      switch (action) {
        case 'switch':
          _git.branch = name;
        case 'delete':
          _git.branches.remove(name);
        case 'merge':
          _git.history.insert(0, (
            id: 'sample-${_git.sequence++}',
            title: '${context.tr('gitMerge')} $name',
            branch: _git.branch,
          ));
          _git.ahead++;
      }
    });
    showResourceNotice(context, 'gitPreview');
  }

  Widget _changes() => Column(
    children: [
      for (final staged in [false, true])
        ExpansionTile(
          trailing: const DisclosureIcon(),
          key: ValueKey(
            '${widget.host}/$_project/$_worktree/$staged/${_git.staged}/${_git.committed}',
          ),
          initiallyExpanded: staged ? _git.staged : true,
          tilePadding: EdgeInsets.zero,
          title: Row(
            children: [
              Text(context.tr(staged ? 'staged' : 'workingTree')),
              const SizedBox(width: 8),
              Text(
                !_git.committed && _git.staged == staged ? '3' : '0',
                style: TextStyle(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
              ),
            ],
          ),
          children: [
            if (_git.committed || _git.staged != staged)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 24),
                child: Text(context.tr('noChanges')),
              )
            else ...[
              ListTile(
                contentPadding: EdgeInsets.zero,
                leading: const AppIcon('file'),
                title: Text(_git.file),
                trailing: const AppIcon('down'),
                onTap: _selectDiff,
              ),
              DiffBlock(file: _git.file),
              const SizedBox(height: 16),
              Row(
                children: [
                  Expanded(
                    child: OutlinedButton(
                      onPressed: () =>
                          setState(() => _git.staged = !_git.staged),
                      child: Text(
                        context.tr(_git.staged ? 'unstage' : 'stage'),
                      ),
                    ),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: FilledButton(
                      onPressed: _git.staged
                          ? _commit
                          : () => pushPage(
                              context,
                              FilesPage(
                                host: widget.host,
                                project: _project,
                                branch: _worktree,
                              ),
                            ),
                      child: Text(
                        context.tr(_git.staged ? 'commit' : 'continueEdit'),
                      ),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 16),
            ],
          ],
        ),
    ],
  );

  Widget _branches() => Column(
    crossAxisAlignment: CrossAxisAlignment.stretch,
    children: [
      Surface(
        padding: const EdgeInsets.symmetric(horizontal: 12),
        child: Column(
          children: [
            for (final branch in _git.branches)
              ListTile(
                contentPadding: EdgeInsets.zero,
                leading: const AppIcon('branch'),
                title: Text(branch),
                subtitle: branch == _git.branch
                    ? Text(context.tr('gitCurrent'))
                    : null,
                trailing: AppIcon(branch == _git.branch ? 'check' : 'chevron'),
                onTap: () => _branchActions(branch),
              ),
          ],
        ),
      ),
      const SizedBox(height: 16),
      OutlinedButton.icon(
        onPressed: _createBranch,
        icon: const AppIcon('plus'),
        label: Text(context.tr('gitCreateBranch')),
      ),
    ],
  );

  Widget _history() => Surface(
    padding: const EdgeInsets.symmetric(horizontal: 12),
    child: Column(
      children: [
        for (final item in _git.history)
          ListTile(
            contentPadding: EdgeInsets.zero,
            leading: const AppIcon('clock'),
            title: Text(item.title),
            subtitle: Text('${item.branch} · ${item.id}'),
            trailing: const AppIcon('chevron'),
            onTap: () => showAppSheet<void>(
              context,
              context.tr('gitHistory'),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    item.title,
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  Text(
                    '${item.branch} · ${item.id}',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                  const SizedBox(height: 16),
                  const DiffBlock(file: 'login.css'),
                ],
              ),
            ),
          ),
      ],
    ),
  );

  @override
  Widget build(BuildContext context) {
    if (AppSession.maybeOf(context) != null) {
      return LiveGitPage(hostId: widget.hostId, worktreeId: widget.worktreeId);
    }
    final colors = Theme.of(context).colorScheme;
    final lines = gitDiffs.values.expand((diff) => diff.lines);
    final added = _git.committed
        ? 0
        : lines.where((line) => line.startsWith('+')).length;
    final removed = _git.committed
        ? 0
        : lines.where((line) => line.startsWith('-')).length;
    return PageFrame(
      title: context.tr('git'),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Semantics(
            label: context.tr('gitActions'),
            button: true,
            child: Surface(
              radius: 16,
              onTap: _actions,
              child: Row(
                children: [
                  const AppIcon('branch'),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Text(
                      _git.branch,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                    ),
                  ),
                  const SizedBox(width: 8),
                  Text('+$added', style: TextStyle(color: colors.tertiary)),
                  const SizedBox(width: 8),
                  Text('−$removed', style: TextStyle(color: colors.secondary)),
                ],
              ),
            ),
          ),
          const SizedBox(height: 16),
          SegmentedButton<String>(
            showSelectedIcon: false,
            segments: [
              for (final entry in [
                ('changes', 'changes'),
                ('branches', 'gitBranches'),
                ('history', 'gitHistory'),
              ])
                ButtonSegment(
                  value: entry.$1,
                  label: Text(context.tr(entry.$2)),
                ),
            ],
            selected: {_tab},
            onSelectionChanged: (value) => setState(() => _tab = value.single),
            style: SegmentedButton.styleFrom(
              side: BorderSide.none,
              selectedBackgroundColor: colors.surfaceContainerHigh,
              backgroundColor: Colors.transparent,
            ),
          ),
          const SizedBox(height: 16),
          switch (_tab) {
            'branches' => _branches(),
            'history' => _history(),
            _ => _changes(),
          },
        ],
      ),
    );
  }
}

class DiffBlock extends StatelessWidget {
  const DiffBlock({super.key, required this.file});
  final String file;

  @override
  Widget build(BuildContext context) {
    final diff = gitDiffs[file]!;
    final colors = Theme.of(context).colorScheme;
    return Surface(
      padding: EdgeInsets.zero,
      radius: 14,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.all(12),
            child: Text(
              diff.path,
              style: const TextStyle(fontFamily: 'monospace', fontSize: 14),
            ),
          ),
          const Divider(),
          SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: IntrinsicWidth(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  for (var index = 0; index < diff.lines.length; index++)
                    Container(
                      color: diff.lines[index].startsWith('+')
                          ? colors.tertiary.withValues(alpha: .10)
                          : diff.lines[index].startsWith('-')
                          ? colors.secondary.withValues(alpha: .10)
                          : null,
                      padding: const EdgeInsets.symmetric(
                        horizontal: 12,
                        vertical: 2,
                      ),
                      child: Text(
                        '${index + 12}  ${diff.lines[index]}',
                        style: TextStyle(
                          fontFamily: 'monospace',
                          fontSize: 14,
                          height: 1.7,
                          color: diff.lines[index].startsWith('+')
                              ? colors.tertiary
                              : diff.lines[index].startsWith('-')
                              ? colors.secondary
                              : colors.onSurface,
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _GitForm extends StatefulWidget {
  const _GitForm({
    required this.label,
    required this.action,
    this.hint,
    this.multiline = false,
    this.validate,
  });
  final String label;
  final String action;
  final String? hint;
  final bool multiline;
  final String? Function(String)? validate;

  @override
  State<_GitForm> createState() => _GitFormState();
}

class _GitFormState extends State<_GitForm> {
  final _form = GlobalKey<FormState>();
  final _controller = TextEditingController();

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Form(
    key: _form,
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        TextFormField(
          controller: _controller,
          autofocus: true,
          minLines: widget.multiline ? 3 : 1,
          maxLines: widget.multiline ? 6 : 1,
          decoration: InputDecoration(
            labelText: context.tr(widget.label),
            hintText: widget.hint == null ? null : context.tr(widget.hint!),
            alignLabelWithHint: true,
          ),
          validator: (value) {
            final text = value?.trim() ?? '';
            if (text.isEmpty) return context.tr(widget.label);
            return widget.validate?.call(text);
          },
        ),
        const SizedBox(height: 16),
        FilledButton(
          onPressed: () {
            if (_form.currentState!.validate()) {
              Navigator.pop(context, _controller.text.trim());
            }
          },
          child: Text(context.tr(widget.action)),
        ),
      ],
    ),
  );
}
