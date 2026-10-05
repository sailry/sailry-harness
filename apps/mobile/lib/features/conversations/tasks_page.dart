import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

import '../../l10n/strings.dart';
import '../../ui/kit.dart';
import '../../ui/theme.dart';
import '../../runtime/session.dart';
import 'live/tasks.dart';
import 'conversation_page.dart';
import 'running_task_frame.dart';

class TasksPage extends StatelessWidget {
  const TasksPage({
    super.key,
    this.initialFilter = 'all',
    this.sessionsOnly = false,
  });
  final String initialFilter;
  final bool sessionsOnly;

  @override
  Widget build(BuildContext context) {
    final session = AppSession.maybeOf(context);
    return session == null
        ? const _PreviewTasksPage()
        : LiveTasksPage(
            session: session,
            initialFilter: initialFilter,
            sessionsOnly: sessionsOnly,
          );
  }
}

class _PreviewTasksPage extends StatefulWidget {
  const _PreviewTasksPage();

  @override
  State<_PreviewTasksPage> createState() => _TasksPageState();
}

class _Task {
  _Task(
    this.id,
    this.title,
    this.note,
    this.status,
    this.project,
    this.host,
    this.branch, {
    this.localized = false,
  });

  final String id;
  final String title;
  final String note;
  final String status;
  final String project;
  final String host;
  final String branch;
  final bool localized;
  bool archived = false;
}

class _TasksPageState extends State<_PreviewTasksPage> {
  final List<_Task> _tasks = [
    _Task(
      'approval',
      'approveTitle',
      'approveNote',
      'waiting',
      'sailry-web',
      'Studio',
      'feature/sign-in',
      localized: true,
    ),
    _Task(
      'question',
      'questionTitle',
      'questionNote',
      'waiting',
      'sailry-api',
      'Build Server',
      'docs/api',
      localized: true,
    ),
    _Task(
      'search',
      'taskSearch',
      'taskSearchNote',
      'running',
      'sailry',
      'Studio',
      'main',
      localized: true,
    ),
    _Task(
      'test',
      'taskTest',
      'taskTestNote',
      'running',
      'sailry',
      'Build Server',
      'fix/recovery',
      localized: true,
    ),
    _Task(
      'done',
      'taskDone',
      'taskDoneNote',
      'completed',
      'sailry-web',
      'Studio',
      'main',
      localized: true,
    ),
  ];
  String _host = 'all';
  String _project = 'all';
  String _filter = 'all';
  String _search = '';
  bool _searching = false;

  String _title(_Task task) =>
      task.localized ? context.tr(task.title) : task.title;
  String _note(_Task task) =>
      task.localized ? context.tr(task.note) : task.note;

  void _projects() {
    final projects = _tasks
        .where((task) => _host == 'all' || task.host == _host)
        .map((task) => task.project)
        .toSet();
    showAppSheet(
      context,
      context.tr('filterProjects'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final project in ['all', ...projects])
            ListTile(
              leading: const AppIcon('folder'),
              title: Text(
                project == 'all' ? context.tr('allProjects') : project,
              ),
              trailing: project == _project ? const AppIcon('check') : null,
              onTap: () {
                setState(() => _project = project);
                Navigator.pop(context);
              },
            ),
        ],
      ),
    );
  }

  Future<void> _create() async {
    var draft = '';
    var host = _host == 'all' ? 'Studio' : _host;
    var project = _project == 'all' ? 'sailry-web' : _project;
    var branch = 'main';
    _Task? created;
    await showAppSheet(
      context,
      context.tr('newTask'),
      child: StatefulBuilder(
        builder: (context, update) => FormBody(
          children: [
            TextFormField(
              initialValue: draft,
              minLines: 3,
              maxLines: 5,
              autofocus: true,
              onChanged: (value) => update(() => draft = value),
              decoration: InputDecoration(hintText: context.tr('describeTask')),
            ),
            SelectField<String>(
              value: host,
              label: context.tr('newTaskHost'),
              options: [
                for (final value in ['Studio', 'Build Server']) (value, value),
              ],
              onChanged: (value) => update(() => host = value),
            ),
            SelectField<String>(
              value: project,
              label: context.tr('newTaskProject'),
              options: [
                for (final value in ['sailry-web', 'sailry-api', 'sailry'])
                  (value, value),
              ],
              onChanged: (value) => update(() => project = value),
            ),
            SelectField<String>(
              value: branch,
              label: context.tr('newTaskWorktree'),
              options: [
                for (final value in ['main', 'feature/sign-in']) (value, value),
              ],
              onChanged: (value) => update(() => branch = value),
            ),
            FilledButton(
              onPressed: draft.trim().isEmpty
                  ? null
                  : () {
                      created = _Task(
                        'created-${_tasks.length}',
                        draft.trim(),
                        context.tr('taskCreated'),
                        'waiting',
                        project,
                        host,
                        branch,
                      );
                      setState(() {
                        _tasks.insert(0, created!);
                        _filter = 'all';
                        _host = 'all';
                        _project = 'all';
                        _search = '';
                        _searching = false;
                      });
                      Navigator.pop(context);
                    },
              child: Text(context.tr('create')),
            ),
          ],
        ),
      ),
    );
    if (mounted && created != null) _open(created!);
  }

  void _open(_Task task) {
    pushPage(
      context,
      ConversationPage(
        title: _title(task),
        project: task.project,
        host: task.host,
        branch: task.branch,
        sample: task.id == 'approval',
        question: task.id == 'question',
      ),
    );
  }

  void _delete(_Task task) {
    showAppSheet(
      context,
      context.tr('deleteTask'),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(_title(task), style: Theme.of(context).textTheme.titleMedium),
          const SizedBox(height: 8),
          Text(context.tr('deleteWarning')),
          const SizedBox(height: 20),
          FilledButton(
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(context).colorScheme.error,
              foregroundColor: Theme.of(context).colorScheme.onError,
            ),
            onPressed: () {
              setState(() => _tasks.remove(task));
              Navigator.pop(context);
            },
            child: Text(context.tr('delete')),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final visible = _tasks.where(
      (task) =>
          task.archived == (_filter == 'archived') &&
          (_filter == 'all' ||
              _filter == 'archived' ||
              task.status == _filter) &&
          (_host == 'all' || task.host == _host) &&
          (_project == 'all' || task.project == _project) &&
          '${_title(task)} ${task.project} ${task.host}'.toLowerCase().contains(
            _search.toLowerCase(),
          ),
    );
    return PageFrame(
      title: context.tr('brand'),
      titleSize: 28,
      actions: [
        RoundButton(
          icon: 'server',
          tooltip: context.tr('selectHost'),
          onPressed: () => showHostPicker(
            context,
            selected: _host,
            allowAll: true,
            onSelected: (value) => setState(() {
              _host = value;
              _project = 'all';
            }),
          ),
        ),
        RoundButton(
          icon: _searching ? 'close' : 'search',
          tooltip: context.tr('searchTasks'),
          onPressed: () => setState(() {
            _searching = !_searching;
            if (!_searching) _search = '';
          }),
        ),
        RoundButton(
          icon: 'plus',
          primary: true,
          tooltip: context.tr('newTask'),
          onPressed: _create,
        ),
      ],
      child: SlidableAutoCloseBehavior(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (_searching) ...[
              TextField(
                autofocus: true,
                decoration: InputDecoration(
                  prefixIcon: const Padding(
                    padding: EdgeInsets.all(12),
                    child: AppIcon('search'),
                  ),
                  hintText: context.tr('searchTasks'),
                ),
                onChanged: (value) => setState(() => _search = value),
              ),
              const SizedBox(height: 16),
            ],
            SelectorCard(
              title: _project == 'all' ? context.tr('allProjects') : _project,
              subtitle: context.tr('allWorktrees'),
              onTap: _projects,
            ),
            const SizedBox(height: 16),
            LayoutBuilder(
              builder: (context, constraints) {
                final tabWidth = (constraints.maxWidth - 12) / 5;
                return SingleChildScrollView(
                  key: const ValueKey('task-filters'),
                  scrollDirection: Axis.horizontal,
                  child: Row(
                    children: [
                      for (final filter in [
                        'all',
                        'waiting',
                        'running',
                        'completed',
                        'archived',
                      ])
                        Padding(
                          padding: EdgeInsets.only(
                            right: filter == 'archived' ? 0 : 3,
                          ),
                          child: Semantics(
                            selected: _filter == filter,
                            child: TextButton(
                              key: ValueKey('task-filter-$filter'),
                              style: TextButton.styleFrom(
                                minimumSize: Size(tabWidth, 44),
                                visualDensity: VisualDensity.standard,
                                padding: const EdgeInsets.symmetric(
                                  horizontal: 2,
                                  vertical: 9,
                                ),
                                tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                                foregroundColor: _filter == filter
                                    ? colors.onSurface
                                    : colors.onSurfaceVariant,
                                backgroundColor: _filter == filter
                                    ? colors.surfaceContainerHigh
                                    : Colors.transparent,
                                side: BorderSide(
                                  color: _filter == filter
                                      ? colors.outlineVariant
                                      : Colors.transparent,
                                ),
                                shape: RoundedRectangleBorder(
                                  borderRadius: BorderRadius.circular(12),
                                ),
                                textStyle: TextStyle(
                                  fontSize: 14,
                                  height: 1.5,
                                  fontWeight: _filter == filter
                                      ? FontWeight.w600
                                      : FontWeight.w400,
                                ),
                              ),
                              onPressed: () => setState(() => _filter = filter),
                              child: Text(
                                context.tr(
                                  filter == 'archived' ? 'archiveTab' : filter,
                                ),
                                softWrap: false,
                              ),
                            ),
                          ),
                        ),
                    ],
                  ),
                );
              },
            ),
            const SizedBox(height: 18),
            if (visible.isEmpty)
              Padding(
                padding: const EdgeInsets.all(30),
                child: Text(
                  context.tr(
                    _filter == 'archived' ? 'archiveEmpty' : 'noResults',
                  ),
                  textAlign: TextAlign.center,
                  style: TextStyle(color: colors.onSurfaceVariant),
                ),
              ),
            for (final task in visible)
              Padding(
                padding: const EdgeInsets.only(bottom: 9),
                child: ClipRRect(
                  borderRadius: BorderRadius.circular(18),
                  child: Slidable(
                    key: ValueKey(task.id),
                    endActionPane: ActionPane(
                      motion: const DrawerMotion(),
                      extentRatio: .46,
                      children: [
                        CustomSlidableAction(
                          onPressed: (_) =>
                              setState(() => task.archived = !task.archived),
                          backgroundColor: colors.secondaryContainer,
                          foregroundColor: colors.onSecondaryContainer,
                          child: Column(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              AppIcon(
                                task.archived ? 'restore' : 'archive',
                                size: 24,
                                color: colors.onSecondaryContainer,
                              ),
                              const SizedBox(height: 4),
                              Text(
                                context.tr(
                                  task.archived ? 'restore' : 'archiveShort',
                                ),
                                overflow: TextOverflow.ellipsis,
                              ),
                            ],
                          ),
                        ),
                        CustomSlidableAction(
                          onPressed: (_) => _delete(task),
                          backgroundColor: colors.error,
                          foregroundColor: colors.onError,
                          child: Column(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              AppIcon('trash', size: 24, color: colors.onError),
                              const SizedBox(height: 4),
                              Text(
                                context.tr('delete'),
                                overflow: TextOverflow.ellipsis,
                              ),
                            ],
                          ),
                        ),
                      ],
                    ),
                    child: RunningTaskFrame(
                      active: task.status == 'running',
                      child: Surface(
                        radius: 18,
                        padding: const EdgeInsets.all(15),
                        onTap: () => _open(task),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Text(
                              _title(task),
                              style: Theme.of(context).textTheme.titleMedium
                                  ?.copyWith(
                                    fontWeight: FontWeight.w600,
                                    height: 1.5,
                                  ),
                            ),
                            const SizedBox(height: 6),
                            Text(
                              '${task.status == 'waiting' ? '•  ' : ''}${_note(task)}',
                              style: TextStyle(
                                height: 1.5,
                                color: SailryTheme.statusColor(
                                  context,
                                  switch (task.status) {
                                    'waiting' => StatusTone.warning,
                                    'completed' => StatusTone.success,
                                    'cancelled' => StatusTone.danger,
                                    _ => StatusTone.neutral,
                                  },
                                ),
                              ),
                            ),
                            const SizedBox(height: 11),
                            Row(
                              children: [
                                Container(
                                  width: 20,
                                  height: 20,
                                  decoration: BoxDecoration(
                                    color: colors.surfaceContainerHigh,
                                    border: Border.all(
                                      color: colors.outlineVariant,
                                    ),
                                    borderRadius: BorderRadius.circular(6),
                                  ),
                                  child: Center(
                                    child: AppIcon(
                                      'folder',
                                      size: 14,
                                      color: colors.onSurfaceVariant,
                                    ),
                                  ),
                                ),
                                const SizedBox(width: 5),
                                Expanded(
                                  child: Text(
                                    '${task.project} · ${task.host}',
                                    style: TextStyle(
                                      height: 1.5,
                                      color: colors.onSurfaceVariant,
                                    ),
                                  ),
                                ),
                              ],
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}
