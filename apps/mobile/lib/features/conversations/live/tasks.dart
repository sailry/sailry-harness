import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import '../../../ui/toast.dart';
import '../../resources/project_form.dart';
import '../../resources/workspace.dart';
import '../../terminal/terminal_page.dart';
import 'create.dart';
import 'task.dart';
import 'presentation.dart';
import 'project_bar.dart';
import 'recency.dart';
import 'task_filters.dart';
import 'terminal_tasks.dart';

class LiveTasksPage extends StatefulWidget {
  const LiveTasksPage({
    super.key,
    required this.session,
    this.initialFilter = 'all',
    this.sessionsOnly = false,
  });
  final AppSession session;
  final String initialFilter;
  final bool sessionsOnly;

  @override
  State<LiveTasksPage> createState() => _LiveTasksPageState();
}

class _LiveTasksPageState extends State<LiveTasksPage> {
  String? _projectHost;
  String? _project;
  late String _filter = widget.initialFilter;
  String _search = '';
  bool _searching = false;
  Timer? _clock;

  @override
  void initState() {
    super.initState();
    // One page clock refreshes relative row times and day sections.
    _clock = Timer.periodic(const Duration(minutes: 1), (_) {
      if (mounted && TickerMode.valuesOf(context).enabled) setState(() {});
    });
  }

  @override
  void dispose() {
    _clock?.cancel();
    super.dispose();
  }

  Future<void> _terminal() async {
    final host = widget.session.selectedHost;
    if (host == null || !host.connected) {
      _error(context.tr('conversationNoHost'));
      return;
    }
    final tree = await showLiveWorkspacePicker(
      context,
      host,
      null,
      project: _projectHost == host.id ? _project : null,
    );
    if (mounted && tree != null) {
      pushPage(
        context,
        TerminalPage(hostId: host.id, worktreeId: tree, createNew: true),
      );
    }
  }

  void _error(Object error) {
    if (!mounted) return;
    showToast(context, failureLabel(error, translate: context.tr));
  }

  Future<void> _create() async {
    final selected = widget.session.selectedHost;
    if (selected == null || !selected.connected) {
      _error(context.tr('conversationNoHost'));
      return;
    }
    pushPage(
      context,
      NewConversationPage(
        host: selected,
        initialProject:
            _projectHost == selected.id && _project != unassignedProject
            ? _project
            : null,
      ),
    );
  }

  Future<void> _archive(
    HostConnection host,
    Map<String, dynamic> session,
  ) async {
    try {
      await host.command('set_session_archived', {
        'session': session['id'],
        'expected_revision': session['revision'],
        'archived': session['archived'] != true,
      });
    } catch (error) {
      _error(error);
    }
  }

  Future<void> _delete(
    HostConnection host,
    Map<String, dynamic> session,
  ) async {
    await showAppSheet(
      context,
      context.tr('deleteTask'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            title(session, translate: context.tr),
            style: Theme.of(context).textTheme.titleMedium,
          ),
          const SizedBox(height: 8),
          Text(context.tr('deleteWarning')),
          const SizedBox(height: 20),
          FilledButton(
            onPressed: () async {
              try {
                await host.command('remove_session', {
                  'session': session['id'],
                  'expected_revision': session['revision'],
                });
                if (mounted && context.mounted) Navigator.pop(context);
              } catch (error) {
                _error(error);
              }
            },
            child: Text(context.tr('delete')),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.session,
    builder: (context, _) {
      final hosts = widget.session.hosts;
      final selected = widget.session.selectedHost;
      final projectId = _projectHost == selected?.id ? _project : null;
      final projects = objects(selected?.snapshot['projects']);
      final selectedProject = projects
          .where((project) => project['id'] == projectId)
          .firstOrNull;
      final tasks = <(HostConnection, Map<String, dynamic>)>[
        for (final host in hosts.where((host) => host.id == selected?.id))
          for (final session in objects(host.snapshot['sessions']))
            if (inProject(projectId, session['project']) &&
                (session['archived'] == true) == (_filter == 'archived') &&
                (_filter == 'all' ||
                    _filter == 'archived' ||
                    status(session) == _filter) &&
                title(
                  session,
                  translate: context.tr,
                ).toLowerCase().contains(_search.toLowerCase()))
              (host, session),
      ];
      final colors = Theme.of(context).colorScheme;
      final now = DateTime.now();
      final groups = groupTasks(tasks, (task) => sessionGroup(task.$2, now));
      final terminals = objects(selected?.snapshot['terminals']).where((
        terminal,
      ) {
        if (widget.sessionsOnly && _filter != 'terminal') return false;
        final tree = objects(
          selected?.snapshot['worktrees'],
        ).where((tree) => tree['id'] == terminal['worktree']).firstOrNull;
        return inProject(projectId, tree?['project']) &&
            (_filter == 'all' ||
                _filter == 'terminal' ||
                terminalStatus(terminal) == _filter) &&
            terminalTitle(
              terminal,
              translate: context.tr,
            ).toLowerCase().contains(_search.toLowerCase());
      }).toList();
      // Tab counts ignore search so they stay stable while typing.
      final counts = <String, int>{};
      var archived = 0;
      for (final session in objects(selected?.snapshot['sessions'])) {
        if (!inProject(projectId, session['project'])) continue;
        if (session['archived'] == true) {
          archived++;
        } else {
          counts.update(status(session), (n) => n + 1, ifAbsent: () => 1);
        }
      }
      for (final terminal in objects(selected?.snapshot['terminals'])) {
        final tree = objects(
          selected?.snapshot['worktrees'],
        ).where((tree) => tree['id'] == terminal['worktree']).firstOrNull;
        if (!inProject(projectId, tree?['project'])) continue;
        counts.update(
          terminalStatus(terminal),
          (n) => n + 1,
          ifAbsent: () => 1,
        );
      }
      return PageFrame(
        title: context.tr('brand'),
        titleSize: 28,
        failure: selected?.connected != true
            ? HostState(added: selected != null)
            : null,
        actions: [
          RoundButton(
            icon: 'server',
            tooltip: context.tr('selectHost'),
            onPressed: hosts.isEmpty
                ? null
                : () => showAppSheet(
                    context,
                    context.tr('selectHost'),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        for (final host in hosts)
                          ListTile(
                            title: Text(host.label),
                            subtitle: Text(
                              context.tr(host.connected ? 'online' : 'offline'),
                            ),
                            trailing: selected?.id == host.id
                                ? const AppIcon('check')
                                : null,
                            onTap: () {
                              setState(() {
                                _project = null;
                              });
                              widget.session.selectHost(host.id);
                              Navigator.pop(context);
                            },
                          ),
                      ],
                    ),
                  ),
          ),
          RoundButton(
            icon: _searching ? 'close' : 'search',
            tooltip: context.tr('searchTasks'),
            onPressed: selected?.connected != true
                ? null
                : () => setState(() {
                    _searching = !_searching;
                    if (!_searching) _search = '';
                  }),
          ),
          RoundButton(
            icon: 'terminal',
            tooltip: context.tr('newTerminal'),
            onPressed: selected?.connected == true ? _terminal : null,
          ),
          RoundButton(
            icon: 'plus',
            tooltip: context.tr('newConversation'),
            onPressed: selected?.connected == true ? _create : null,
          ),
        ],
        empty: selected?.connected == true && tasks.isEmpty && terminals.isEmpty
            ? EmptyState(
                icon: 'chat',
                message: context.tr(
                  _filter == 'archived'
                      ? 'archiveEmpty'
                      : 'conversationNoTasks',
                ),
              )
            : null,
        child: SlidableAutoCloseBehavior(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (_searching) ...[
                TextField(
                  autofocus: true,
                  decoration: InputDecoration(
                    hintText: context.tr('searchTasks'),
                    prefixIcon: const Padding(
                      padding: EdgeInsets.all(12),
                      child: AppIcon('search'),
                    ),
                  ),
                  onChanged: (value) => setState(() => _search = value),
                ),
                const SizedBox(height: 16),
              ],
              ProjectBar(
                project: selectedProject,
                title: projectId == unassignedProject
                    ? context.tr('unassignedChats')
                    : null,
                leading: projectId == unassignedProject
                    ? const AppIcon('chat', size: 15)
                    : null,
                onTap: () => showAppSheet(
                  context,
                  context.tr('filterProjects'),
                  actions: [
                    if (selected?.connected == true)
                      ListTile(
                        leading: const AppIcon('plus'),
                        title: Text(context.tr('hostRegisterProject')),
                        onTap: () {
                          Navigator.pop(context);
                          showAppSheet(
                            context,
                            context.tr('hostRegisterProject'),
                            child: ProjectForm(host: selected!),
                          );
                        },
                      ),
                  ],
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      ListTile(
                        leading: const AppIcon('folder'),
                        title: Text(context.tr('allProjects')),
                        onTap: () {
                          setState(() => _project = null);
                          Navigator.pop(context);
                        },
                      ),
                      ListTile(
                        leading: const AppIcon('chat'),
                        title: Text(context.tr('unassignedChats')),
                        onTap: () {
                          setState(() {
                            _projectHost = selected?.id;
                            _project = unassignedProject;
                          });
                          Navigator.pop(context);
                        },
                      ),
                      for (final project in projects)
                        ListTile(
                          leading: ProjectIcon(project: project),
                          title: Text(project['name'] as String),
                          onTap: () {
                            setState(() {
                              _projectHost = selected?.id;
                              _project = project['id'] as String;
                            });
                            Navigator.pop(context);
                          },
                        ),
                    ],
                  ),
                ),
              ),
              const SizedBox(height: 10),
              if (taskFilters.contains(_filter))
                TaskFilters(
                  selected: _filter,
                  counts: counts,
                  onSelected: (filter) => setState(() => _filter = filter),
                )
              else
                TaskScope(
                  label: context.tr(
                    _filter == 'archived' ? 'archiveTab' : 'terminal',
                  ),
                  onClose: () => setState(() => _filter = 'all'),
                ),
              const SizedBox(height: 8),
              for (final (group, items) in groups) ...[
                _GroupTitle(context.tr(group.key)),
                for (final (index, (host, session)) in items.indexed)
                  Slidable(
                    key: ValueKey('${host.id}-${session['id']}'),
                    endActionPane: ActionPane(
                      motion: const DrawerMotion(),
                      extentRatio: .46,
                      children: [
                        SlidableAction(
                          onPressed: host.connected
                              ? (_) => _archive(host, session)
                              : null,
                          backgroundColor: colors.secondaryContainer,
                          foregroundColor: colors.onSecondaryContainer,
                          icon: Icons.archive_outlined,
                          label: context.tr(
                            session['archived'] == true
                                ? 'restore'
                                : 'archiveShort',
                          ),
                        ),
                        SlidableAction(
                          onPressed: host.connected
                              ? (_) => _delete(host, session)
                              : null,
                          backgroundColor: colors.error,
                          foregroundColor: colors.onError,
                          icon: Icons.delete_outline,
                          label: context.tr('delete'),
                        ),
                      ],
                    ),
                    child: ConversationTask(
                      key: ValueKey('preview-${host.id}-${session['id']}'),
                      host: host,
                      session: session,
                      now: now,
                      divider: index < items.length - 1,
                      project:
                          projects
                              .where(
                                (project) =>
                                    project['id'] == session['project'],
                              )
                              .firstOrNull ??
                          {},
                    ),
                  ),
              ],
              if (selected != null && terminals.isNotEmpty) ...[
                _GroupTitle(context.tr('terminal')),
                for (final (index, terminal) in terminals.indexed)
                  TerminalTask(
                    host: selected,
                    terminal: terminal,
                    divider: index < terminals.length - 1,
                  ),
              ],
              if (_filter != 'archived' && archived > 0)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Center(
                    child: TextButton(
                      key: const ValueKey('task-archive'),
                      onPressed: () => setState(() => _filter = 'archived'),
                      child: Text(
                        context
                            .tr('archivedCount')
                            .replaceAll('{count}', '$archived'),
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      );
    },
  );
}

class _GroupTitle extends StatelessWidget {
  const _GroupTitle(this.label);
  final String label;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: 14, bottom: 2),
    child: Text(
      label,
      style: TextStyle(
        fontSize: 13,
        color: Theme.of(
          context,
        ).colorScheme.onSurfaceVariant.withValues(alpha: .65),
      ),
    ),
  );
}
