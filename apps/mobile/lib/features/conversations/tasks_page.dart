import 'package:flutter/material.dart';
import '../../runtime/session.dart';
import 'live/tasks.dart';

class TasksPage extends StatelessWidget {
  const TasksPage({
    super.key,
    this.initialFilter = 'all',
    this.sessionsOnly = false,
  });
  final String initialFilter;
  final bool sessionsOnly;
  @override
  Widget build(BuildContext context) => LiveTasksPage(
    session: AppSession.of(context),
    initialFilter: initialFilter,
    sessionsOnly: sessionsOnly,
  );
}
