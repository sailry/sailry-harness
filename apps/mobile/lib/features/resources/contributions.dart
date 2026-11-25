import 'package:flutter/widgets.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart';
import 'file_draft.dart';
import 'overview_cards.dart';

typedef ProjectResourceView = ({
  HostConnection host,
  String worktree,
  Future<void> Function(Widget page) open,
  int revision,
});

typedef ProjectResourceEntry =
    ResourceEntry Function(BuildContext context, ProjectResourceView view);
typedef ProjectResourceHeader =
    Widget Function(ProjectResourceView view, Widget Function(String?) heading);
typedef ProjectResourceAction =
    Widget Function(BuildContext context, ProjectResourceView view);
typedef OpenDirectory =
    Future<void> Function(
      BuildContext context,
      HostConnection host,
      String worktree,
      String directory,
    );

typedef ProjectResourceLink = ({
  HostConnection host,
  String worktree,
  String? session,
  String? kind,
  Object? data,
  Uri? uri,
  Map<String, FileDocument>? documents,
});

typedef OpenProjectResourceLink =
    Future<void> Function(BuildContext context, ProjectResourceLink link);

/// A target match takes priority over a scheme match. The first match owns
/// the open, including any failure. File/directory data is Node-relative.
class ProjectLinkContribution {
  const ProjectLinkContribution.target(String this.kind, this.open)
    : scheme = null;
  const ProjectLinkContribution.scheme(String this.scheme, this.open)
    : kind = null;

  final String? kind;
  final String? scheme;
  final OpenProjectResourceLink open;
}

/// A task list view another page opens on the task tab.
@immutable
class TaskBrowse {
  const TaskBrowse.sessions() : filter = 'all', sessionsOnly = true;
  const TaskBrowse.filter(this.filter) : sessionsOnly = false;

  final String filter;
  final bool sessionsOnly;
}

typedef HostResourceView = ({
  HostConnection host,

  /// Null when the assembly has no task tab.
  ValueChanged<TaskBrowse>? browseTasks,
});

/// One cell of the host page's resource counts.
class HostResourceCount {
  const HostResourceCount({
    required this.id,
    required this.label,
    required this.count,
    required this.open,
    this.changes,
  });

  final String id;

  /// Localization key.
  final String label;
  final int Function(HostConnection host) count;
  final VoidCallback? Function(BuildContext context, HostResourceView view)
  open;
  final Listenable Function(HostConnection host)? changes;
}

typedef TaskSectionQuery = ({
  String filter,
  String search,
  bool Function(Object? project) includesProject,
  Translator translate,
});

/// Records a feature adds to the task list after conversations.
class TaskListSection {
  const TaskListSection({
    required this.scope,
    required this.records,
    required this.include,
    required this.row,
  });

  /// Filter that lists only this section; also its title's localization key.
  final String scope;
  final List<Map<String, dynamic>> Function(HostConnection host) records;
  final bool Function(
    HostConnection host,
    Map<String, dynamic> record,
    TaskSectionQuery query,
  )
  include;
  final Widget Function(
    BuildContext context,
    Key key,
    HostConnection host,
    Map<String, dynamic> record, {
    required bool divider,
  })
  row;
}

class ProjectResourceFeature {
  const ProjectResourceFeature({
    this.entry,
    this.entryChanges,
    this.header,
    this.action,
    this.openDirectory,
    this.links = const [],
    this.hostCount,
    this.taskSection,
  });

  final ProjectResourceEntry? entry;
  final Listenable Function(ProjectResourceView)? entryChanges;
  final ProjectResourceHeader? header;
  final ProjectResourceAction? action;
  final OpenDirectory? openDirectory;
  final List<ProjectLinkContribution> links;
  final HostResourceCount? hostCount;
  final TaskListSection? taskSection;
}

class ProjectResourcesScope extends InheritedWidget {
  const ProjectResourcesScope({
    super.key,
    required this.features,
    required super.child,
  });

  final List<ProjectResourceFeature> features;

  static List<ProjectResourceFeature> of(BuildContext context) =>
      context
          .dependOnInheritedWidgetOfExactType<ProjectResourcesScope>()
          ?.features ??
      const [];

  @override
  bool updateShouldNotify(ProjectResourcesScope oldWidget) =>
      features != oldWidget.features;
}
