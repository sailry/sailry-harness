import 'dart:async';

import 'package:flutter/foundation.dart' show mapEquals, setEquals;
import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/catalog.dart';
import '../../../runtime/json.dart';
import '../../../runtime/query_cache.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import '../../../ui/swipe_action.dart';
import '../../../ui/toast.dart';
import '../../resources/host/host_sheet.dart';
import '../../resources/pair_form.dart';
import '../../resources/project_form.dart';
import '../../resources/scope_picker.dart';
import '../../resources/workspace.dart';
import 'create.dart';
import 'host_status_bar.dart';
import 'message_data.dart' show searchExcerpt;
import 'task.dart';
import 'presentation.dart';
import 'project_bar.dart';
import 'recency.dart';
import 'task_filters.dart';
import 'terminal_tasks.dart';
import '../../../ui/theme.dart';

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

  /// Sessions whose locally indexed messages contain the search, with the
  /// text that matched, so a search also finds content while offline.
  Map<(String, String), String> _contentMatches = const {};
  ({QueryCache cache, HostConnection host, String query, Set<String> sessions})?
  _contentSearch;
  int _contentQuery = 0;
  StreamSubscription<void>? _indexChanges;
  Timer? _clock;
  ModalRoute<dynamic>? _route;
  Animation<double>? _cover;
  bool _ticking = true;
  bool _catalogDirty = true;
  Widget? _catalog;
  (AppLocalizations?, ColorScheme)? _catalogAppearance;

  bool get _previewActive =>
      _ticking && _route?.isCurrent != false && _cover?.isDismissed != false;

  bool _admitPreview() {
    if (_previewActive) return true;
    // A deferred preview also needs an update when the catalog itself is unchanged.
    _catalogDirty = true;
    return false;
  }

  @override
  void initState() {
    super.initState();
    widget.session.addListener(_changed);
    // One page clock refreshes relative row times and day sections.
    _clock = Timer.periodic(const Duration(minutes: 1), (_) => _changed());
  }

  @override
  void didUpdateWidget(LiveTasksPage oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.session != widget.session) {
      oldWidget.session.removeListener(_changed);
      widget.session.addListener(_changed);
      _bindSearch();
      _searchContent();
      _catalogDirty = true;
    }
    if (oldWidget.sessionsOnly != widget.sessionsOnly) _catalogDirty = true;
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
    _catalogDirty = true;
    if (_searching) _searchContent();
    if (mounted && _previewActive) setState(() {});
  }

  void _uncovered(AnimationStatus status) {
    if (status.isDismissed && mounted && _previewActive) setState(() {});
  }

  void _updateControls(VoidCallback update) {
    setState(() {
      update();
      _catalogDirty = true;
    });
  }

  void _setSearching(bool searching) {
    _updateControls(() {
      _searching = searching;
      if (!searching) _search = '';
    });
    _bindSearch();
    _searchContent();
  }

  void _setFilter(String filter) {
    _updateControls(() => _filter = filter);
    _searchContent();
  }

  void _bindSearch() {
    unawaited(_indexChanges?.cancel());
    _indexChanges = _searching
        ? widget.session.cache?.searchChanges.listen(
            (_) => _searchContent(indexChanged: true),
          )
        : null;
  }

  void _searchContent({bool indexChanged = false}) {
    final cache = widget.session.cache;
    final host = widget.session.selectedHost;
    final query = _search.trim();
    if (cache == null || host == null || host.isDemo || query.isEmpty) {
      if (_contentSearch != null) {
        _contentQuery++;
        _contentSearch = null;
        _contentMatches = const {};
      }
      return;
    }
    final sessions = <String>{
      for (final session in objects(host.snapshot['sessions']))
        if (_includesSession(session, _laneOf(host, session)))
          text(session['id']),
    };
    final previous = _contentSearch;
    final sameSearch =
        previous != null &&
        previous.cache == cache &&
        previous.host == host &&
        previous.query == query &&
        setEquals(previous.sessions, sessions);
    if (sameSearch && !indexChanged) return;
    _contentSearch = (
      cache: cache,
      host: host,
      query: query,
      sessions: sessions,
    );
    if (!sameSearch) _contentMatches = const {};
    final request = ++_contentQuery;
    if (sessions.isEmpty) return;
    unawaited(
      cache.search(query, scope: host.cacheScope, sessions: sessions).then((
        hits,
      ) {
        if (!mounted || request != _contentQuery) return;
        final matches = {
          for (final hit in hits)
            (host.id, hit.session): searchExcerpt(hit.body, query),
        };
        if (!mapEquals(_contentMatches, matches)) {
          _updateControls(() => _contentMatches = matches);
        }
      }),
    );
  }

  @override
  void dispose() {
    _contentQuery++;
    unawaited(_indexChanges?.cancel());
    _clock?.cancel();
    widget.session.removeListener(_changed);
    _cover?.removeStatusListener(_uncovered);
    super.dispose();
  }

  bool _includes(Object? project) =>
      inProject(widget.session.selectedProjectId, project);

  bool _includesSession(Map<String, dynamic> session, String lane) =>
      ordinarySession(session) &&
      _includes(session['project']) &&
      (session['archived'] == true) == (_filter == 'archived') &&
      (_filter == 'all' || _filter == 'archived' || lane == _filter);

  static String _laneOf(HostConnection host, Map<String, dynamic> session) =>
      host.sessionLanes[text(session['id'])] ?? 'idle';

  Future<void> _refresh() async {
    await widget.session.selectedHost?.refresh();
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

  void _pair() => showAppSheet<void>(
    context,
    context.tr('pairTitle'),
    child: PairForm(session: widget.session),
  );

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
    final appearance = (
      Localizations.of<AppLocalizations>(context, AppLocalizations),
      Theme.of(context).colorScheme,
    );
    if (_catalog != null &&
        (!_previewActive ||
            (!_catalogDirty && _catalogAppearance == appearance))) {
      return _catalog!;
    }
    _catalogDirty = !_previewActive;
    _catalogAppearance = appearance;
    return _catalog = _buildCatalog(context, context.tr, appearance.$2);
  }

  Widget _buildCatalog(
    BuildContext context,
    Translator translate,
    ColorScheme colors,
  ) {
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
        if (_includesSession(session, _laneOf(host, session)) &&
            (title(
                  session,
                  translate: context.tr,
                ).toLowerCase().contains(_search.toLowerCase()) ||
                _contentMatches.containsKey((host.id, session['id']))))
          (host, session),
    ];
    final now = DateTime.now();
    final groups = groupTasks(
      tasks,
      (task) => sessionGroup(task.$2, now, lane: _laneOf(task.$1, task.$2)),
    );
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
    final hasHosts = widget.session.hosts.isNotEmpty;
    // A project belongs to the selected host; the unscoped filters summarize
    // every paired host.
    final statusHosts = projectId == null || projectId == unassignedProject
        ? widget.session.hosts
        : hosts;
    // Search uses the list's scroll view so its focused field stays mounted.
    final controls = Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ProjectBar(
          searching: _searching,
          onSearch: connected || hasCatalog || _searching
              ? () => _setSearching(!_searching)
              : null,
          onQuery: (value) {
            _updateControls(() => _search = value);
            _searchContent();
          },
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
                ScopeRow(
                  leading: const AppIcon('plus', size: scopeIconSize),
                  title: context.tr('hostRegisterProject'),
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
            child: ScopePicker(
              rows: [
                ScopeRow(
                  leading: const AppIcon('folder', size: scopeIconSize),
                  title: context.tr('allProjects'),
                  selected: selected != null && projectId == null,
                  onTap: selected == null
                      ? null
                      : () {
                          widget.session.selectProject(selected.id, null);
                          Navigator.pop(context);
                        },
                ),
                ScopeRow(
                  leading: const AppIcon('chat', size: scopeIconSize),
                  title: context.tr('unassignedChats'),
                  selected: selected != null && projectId == unassignedProject,
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
              ],
              sections: [
                for (final host in widget.session.resourceHosts)
                  if (projects.any((entry) => entry.$1.id == host.id))
                    (
                      host: host,
                      expanded: host.id == selected?.id,
                      children: [
                        for (final (owner, project) in projects)
                          if (owner.id == host.id)
                            ScopeRow(
                              nested: true,
                              leading: ProjectIcon(
                                project: project,
                                size: scopeIconSize,
                              ),
                              title: project['name'] as String,
                              selected:
                                  host.id == selected?.id &&
                                  project['id'] == projectId,
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
              ],
            ),
          ),
        ),
        const SizedBox(height: 10),
        if (taskFilters.contains(_filter) || _filter == 'archived')
          TaskFilters(
            selected: _filter,
            onSelected: _setFilter,
            onArchive: () =>
                _setFilter(_filter == 'archived' ? 'all' : 'archived'),
          )
        else
          TaskScope(
            label: context.tr(
              _filter == 'archived' ? 'archiveTab' : 'terminal',
            ),
            onClose: () => _setFilter('all'),
          ),
      ],
    );
    final rows = <(Key, Widget Function(Key))>[
      for (final (group, items) in groups) ...[
        (ValueKey(group), (key) => _GroupTitle(translate(group.key), key: key)),
        for (final (index, (host, session)) in items.indexed)
          (
            ValueKey((host.id, session['id'])),
            (key) => _sessionRow(
              key,
              host,
              session,
              colors: colors,
              translate: translate,
              lane: _laneOf(host, session),
              now: now,
              divider: index < items.length - 1,
            ),
          ),
      ],
      if (terminals.isNotEmpty) ...[
        (
          const ValueKey('terminals'),
          (key) => _GroupTitle(translate('terminal'), key: key),
        ),
        for (final (index, (host, terminal)) in terminals.indexed)
          (
            ValueKey(('terminal', host.id, terminal['id'])),
            (key) => _terminalRow(
              key,
              host,
              terminal,
              colors: colors,
              translate: translate,
              divider: index < terminals.length - 1,
            ),
          ),
      ],
    ];
    final rowIndices = {
      for (final (index, (key, _)) in rows.indexed) key: index,
    };
    return PageFrame(
      title: context.tr('brand'),
      subtitle: statusHosts.isEmpty
          ? null
          : HostStatusBar(
              hosts: statusHosts,
              onTap: () => showHostSheet(context, onPair: _pair),
            ),
      onRefresh: hosts.isEmpty ? null : _refresh,
      failure: !connected && !hasCatalog
          ? HostState(
              added: hasHosts,
              failure: selected?.error,
              showFailureReason: true,
              action: !hasHosts && widget.session.ready
                  ? FilledButton(
                      onPressed: _pair,
                      child: Text(context.tr('pair')),
                    )
                  : null,
            )
          : null,
      actions: [
        if (hasHosts)
          RoundButton(
            icon: 'server',
            tooltip: context.tr('selectHost'),
            onPressed: () async {
              final id = await showHostPicker(
                context,
                widget.session,
                selected: selected?.id,
              );
              if (id != null) widget.session.selectHost(id);
            },
          ),
        if (creationHost?.connected == true)
          RoundButton(
            icon: 'plus',
            tooltip: context.tr('newConversation'),
            onPressed: _create,
          ),
      ],
      empty: (connected || hasCatalog) && !hasRecords
          ? EmptyState(
              icon: 'chat',
              message: context.tr(
                _filter == 'archived' ? 'archiveEmpty' : 'conversationNoTasks',
              ),
            )
          : null,
      pinned: _searching ? null : controls,
      sliver: SlidableAutoCloseBehavior(
        child: SliverMainAxisGroup(
          slivers: [
            if (_searching)
              SliverPadding(
                padding: const EdgeInsets.only(bottom: 8),
                sliver: SliverToBoxAdapter(child: controls),
              ),
            SliverList.builder(
              itemCount: rows.length,
              findChildIndexCallback: (key) => rowIndices[key],
              itemBuilder: (_, index) {
                final (key, build) = rows[index];
                return build(key);
              },
            ),
          ],
        ),
      ),
    );
  }

  Widget _sessionRow(
    Key key,
    HostConnection host,
    Map<String, dynamic> session, {
    required ColorScheme colors,
    required Translator translate,
    required String lane,
    required DateTime now,
    required bool divider,
  }) {
    return Slidable(
      key: key,
      endActionPane: ActionPane(
        motion: const DrawerMotion(),
        extentRatio: .46,
        children: [
          SwipeAction(
            onPressed: host.connected ? (_) => _archive(host, session) : null,
            color: colors.secondaryContainer,
            foreground: colors.onSecondaryContainer,
            icon: Icons.archive_outlined,
            label: translate(
              session['archived'] == true ? 'restore' : 'archiveShort',
            ),
          ),
          SwipeAction(
            onPressed: host.connected ? (_) => _delete(host, session) : null,
            color: colors.error,
            foreground: colors.onError,
            icon: Icons.delete_outline,
            label: translate('delete'),
          ),
        ],
      ),
      child: ConversationTask(
        key: ValueKey(('preview', host.id, session['id'])),
        host: host,
        session: session,
        lane: lane,
        now: now,
        admitPreview: _admitPreview,
        match: _searching ? _contentMatches[(host.id, session['id'])] : null,
        divider: divider,
        project:
            objects(host.snapshot['projects'])
                .where((project) => project['id'] == session['project'])
                .firstOrNull ??
            {},
      ),
    );
  }

  Widget _terminalRow(
    Key key,
    HostConnection host,
    Map<String, dynamic> terminal, {
    required ColorScheme colors,
    required Translator translate,
    required bool divider,
  }) {
    return Slidable(
      key: key,
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
            label: translate('close'),
          ),
        ],
      ),
      child: TerminalTask(host: host, terminal: terminal, divider: divider),
    );
  }
}

class _GroupTitle extends StatelessWidget {
  const _GroupTitle(this.label, {super.key});
  final String label;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: 14, bottom: 2),
    child: Text(
      label,
      style: TextStyle(
        fontSize: TypeScale.secondary,
        color: Theme.of(
          context,
        ).colorScheme.onSurfaceVariant.withValues(alpha: .65),
      ),
    ),
  );
}
