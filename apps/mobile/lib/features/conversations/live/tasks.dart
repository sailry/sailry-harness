import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/catalog.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import '../../../ui/swipe_action.dart';
import '../../../ui/toast.dart';
import '../../resources/host/host_sheet.dart';
import '../../resources/pair_form.dart';
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
  late String _filter = widget.initialFilter;
  String _search = '';
  bool _searching = false;
  Timer? _clock;
  ModalRoute<dynamic>? _route;
  Animation<double>? _cover;
  bool _ticking = true;
  Widget? _catalog;

  bool get _previewActive =>
      _ticking && _route?.isCurrent != false && _cover?.isDismissed != false;

  @override
  void initState() {
    super.initState();
    widget.session.addListener(_changed);
    // One page clock refreshes relative row times and day sections.
    _clock = Timer.periodic(const Duration(minutes: 1), (_) {
      if (mounted && _previewActive) setState(() {});
    });
  }

  @override
  void didUpdateWidget(LiveTasksPage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.session != widget.session) {
      oldWidget.session.removeListener(_changed);
      widget.session.addListener(_changed);
    }
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _ticking = TickerMode.valuesOf(context).enabled;
    _route = ModalRoute.of(context);
    final cover = _route?.secondaryAnimation;
    if (!identical(cover, _cover)) {
      _cover?.removeStatusListener(_uncovered);
      _cover = cover;
      _cover?.addStatusListener(_uncovered);
    }
  }

  void _changed() {
    if (mounted && _previewActive) setState(() {});
  }

  void _uncovered(AnimationStatus status) {
    if (status.isDismissed) _changed();
  }

  @override
  void dispose() {
    _clock?.cancel();
    widget.session.removeListener(_changed);
    _cover?.removeStatusListener(_uncovered);
    super.dispose();
  }

  bool _includes(Object? project) =>
      inProject(widget.session.selectedProjectId, project);

  Future<void> _refresh() async {
    await widget.session.selectedHost?.refresh();
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
      widget.session.selectedWorktreeId,
      project: widget.session.selectedProjectId,
    );
    if (mounted && tree != null) {
      widget.session.selectWorkspace(host.id, tree);
      pushPage(
        context,
        TerminalPage(hostId: host.id, worktreeId: tree, createNew: true),
      );
    }
  }

  Future<void> _closeTerminal(
    HostConnection host,
    Map<String, dynamic> terminal,
  ) async {
    try {
      await closeTerminal(host, terminal);
    } catch (error) {
      _error(error);
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
        initialProject: widget.session.selectedProjectId,
        initialWorktree: widget.session.selectedWorktreeId,
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
  Widget build(BuildContext context) {
    // SessionScope may rebuild the parent while another route is visible. Keep
    // the existing rows through that route and its return transition, then
    // project the latest catalog once without replaying intermediate updates.
    if (!_previewActive && _catalog != null) return _catalog!;
    return _catalog = _buildCatalog(context);
  }

  Widget _buildCatalog(BuildContext context) {
    final selected = widget.session.selectedHost;
    final hosts = [?selected];
    final projectId = widget.session.selectedProjectId;
    final projects = hostRecords(widget.session.resourceHosts, 'projects');
    final sessions = hostRecords(hosts, 'sessions');
    final allTerminals = hostRecords(hosts, 'terminals');
    final creationHost = selected;
    final selectedProject = projects
        .where(
          (entry) => entry.$1.id == selected?.id && entry.$2['id'] == projectId,
        )
        .firstOrNull
        ?.$2;
    final tasks = <HostRecord>[
      for (final (host, session) in sessions)
        if (ordinarySession(session) &&
            _includes(session['project']) &&
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
    Object? terminalProject(
      HostConnection host,
      Map<String, dynamic> terminal,
    ) => objects(host.snapshot['worktrees'])
        .where((tree) => tree['id'] == terminal['worktree'])
        .firstOrNull?['project'];
    final terminals = <HostRecord>[
      for (final (host, terminal) in allTerminals)
        if (!(widget.sessionsOnly && _filter != 'terminal') &&
            activeTerminal(terminal) &&
            _includes(terminalProject(host, terminal)) &&
            (_filter == 'all' ||
                _filter == 'terminal' ||
                terminalStatus(terminal) == _filter) &&
            terminalTitle(
              terminal,
              translate: context.tr,
            ).toLowerCase().contains(_search.toLowerCase()))
          (host, terminal),
    ];
    final hasRecords = tasks.isNotEmpty || terminals.isNotEmpty;
    final hasCatalog =
        projects.any((entry) => entry.$1.id == selected?.id) ||
        sessions.isNotEmpty ||
        allTerminals.isNotEmpty;
    final connected = hosts.any((host) => host.connected);
    return PageFrame(
      title: context.tr('brand'),
      titleSize: 28,
      onRefresh: hosts.isEmpty ? null : _refresh,
      failure: !connected && !hasCatalog
          ? HostState(
              added: selected != null,
              failure: selected?.error,
              // The first screen offers pairing, so an offline or missing
              // host is not a dead end.
              action: FilledButton(
                onPressed: widget.session.ready
                    ? () => showAppSheet<void>(
                        context,
                        context.tr('pairTitle'),
                        child: PairForm(session: widget.session),
                      )
                    : null,
                child: Text(context.tr('pair')),
              ),
            )
          : null,
      actions: [
        if (widget.session.hosts.isNotEmpty)
          RoundButton(
            icon: 'server',
            tooltip: context.tr('selectHost'),
            onPressed: () => showAppSheet(
              context,
              context.tr('selectHost'),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  for (final host in widget.session.hosts)
                    HostRow(
                      host: host,
                      selected: selected?.id == host.id,
                      onTap: () {
                        widget.session.selectHost(host.id);
                        Navigator.pop(context);
                      },
                    ),
                ],
              ),
            ),
          ),
        if (connected || hasCatalog || _searching)
          RoundButton(
            icon: _searching ? 'close' : 'search',
            tooltip: context.tr('searchTasks'),
            onPressed: () => setState(() {
              _searching = !_searching;
              if (!_searching) _search = '';
            }),
          ),
        if (creationHost?.connected == true) ...[
          RoundButton(
            icon: 'terminal',
            tooltip: context.tr('newTerminal'),
            onPressed: _terminal,
          ),
          RoundButton(
            icon: 'plus',
            tooltip: context.tr('newConversation'),
            onPressed: _create,
          ),
        ],
      ],
      empty: (connected || hasCatalog) && !hasRecords
          ? EmptyState(
              icon: 'chat',
              message: context.tr(
                _filter == 'archived' ? 'archiveEmpty' : 'conversationNoTasks',
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
                  if (creationHost?.connected == true)
                    ListTile(
                      leading: const AppIcon('plus'),
                      title: Text(context.tr('hostRegisterProject')),
                      onTap: () {
                        Navigator.pop(context);
                        showAppSheet(
                          context,
                          context.tr('hostRegisterProject'),
                          child: ProjectForm(host: creationHost!),
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
                      onTap: selected == null
                          ? null
                          : () {
                              widget.session.selectProject(selected.id, null);
                              Navigator.pop(context);
                            },
                    ),
                    ListTile(
                      leading: const AppIcon('chat'),
                      title: Text(context.tr('unassignedChats')),
                      onTap: selected == null
                          ? null
                          : () {
                              widget.session.selectProject(
                                selected.id,
                                unassignedProject,
                              );
                              Navigator.pop(context);
                            },
                    ),
                    for (final (host, project) in projects)
                      ListTile(
                        leading: ProjectIcon(project: project),
                        title: Text(project['name'] as String),
                        onTap: () {
                          widget.session.selectProject(
                            host.id,
                            project['id'] as String,
                          );
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
                onSelected: (filter) => setState(() => _filter = filter),
                onArchive: () => setState(() => _filter = 'archived'),
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
                  key: ValueKey((host.id, session['id'])),
                  endActionPane: ActionPane(
                    motion: const DrawerMotion(),
                    extentRatio: .46,
                    children: [
                      SwipeAction(
                        onPressed: host.connected
                            ? (_) => _archive(host, session)
                            : null,
                        color: colors.secondaryContainer,
                        foreground: colors.onSecondaryContainer,
                        icon: Icons.archive_outlined,
                        label: context.tr(
                          session['archived'] == true
                              ? 'restore'
                              : 'archiveShort',
                        ),
                      ),
                      SwipeAction(
                        onPressed: host.connected
                            ? (_) => _delete(host, session)
                            : null,
                        color: colors.error,
                        foreground: colors.onError,
                        icon: Icons.delete_outline,
                        label: context.tr('delete'),
                      ),
                    ],
                  ),
                  child: ConversationTask(
                    key: ValueKey(('preview', host.id, session['id'])),
                    host: host,
                    session: session,
                    now: now,
                    previewActive: () => _previewActive,
                    divider: index < items.length - 1,
                    project:
                        objects(host.snapshot['projects'])
                            .where(
                              (project) => project['id'] == session['project'],
                            )
                            .firstOrNull ??
                        {},
                  ),
                ),
            ],
            if (terminals.isNotEmpty) ...[
              _GroupTitle(context.tr('terminal')),
              for (final (index, (host, terminal)) in terminals.indexed)
                Slidable(
                  key: ValueKey(('terminal', host.id, terminal['id'])),
                  endActionPane: ActionPane(
                    motion: const DrawerMotion(),
                    extentRatio: .24,
                    children: [
                      SwipeAction(
                        onPressed: host.connected
                            ? (_) => _closeTerminal(host, terminal)
                            : null,
                        color: colors.error,
                        foreground: colors.onError,
                        icon: Icons.close,
                        label: context.tr('close'),
                      ),
                    ],
                  ),
                  child: TerminalTask(
                    host: host,
                    terminal: terminal,
                    divider: index < terminals.length - 1,
                  ),
                ),
            ],
          ],
        ),
      ),
    );
  }
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
