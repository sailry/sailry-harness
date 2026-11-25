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

/// A target match takes priority over a scheme match and owns the open,
/// including any failure. File/directory data is Node-relative.
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

class ProjectResources {
  ProjectResources(Iterable<ProjectResourceFeature> features)
    : features = List.unmodifiable(features) {
    for (final feature in this.features) {
      for (final link in feature.links) {
        _register(link);
      }
      if (feature.openDirectory case final open?) {
        _register(
          ProjectLinkContribution.target(
            'directory',
            (context, link) =>
                open(context, link.host, link.worktree, link.data as String),
          ),
        );
      }
    }
  }

  final List<ProjectResourceFeature> features;
  final _handlers =
      <({String? kind, String? scheme}), OpenProjectResourceLink>{};

  void _register(ProjectLinkContribution contribution) {
    final kind = contribution.kind;
    final scheme = contribution.scheme?.toLowerCase();
    final route = (kind: kind, scheme: scheme);
    if (_handlers.containsKey(route)) {
      final label = kind == null ? 'scheme/$scheme' : 'target/$kind';
      throw ArgumentError('Duplicate project link route $label');
    }
    _handlers[route] = contribution.open;
  }

  OpenProjectResourceLink? _handler(String? kind, String? scheme) =>
      (kind == null ? null : _handlers[(kind: kind, scheme: null)]) ??
      (scheme == null || scheme.isEmpty
          ? null
          : _handlers[(kind: null, scheme: scheme)]);
}

class ProjectResourcesScope extends InheritedWidget {
  ProjectResourcesScope({
    super.key,
    required List<ProjectResourceFeature> features,
    required super.child,
  }) : resources = ProjectResources(features);

  const ProjectResourcesScope.registered({
    super.key,
    required this.resources,
    required super.child,
  });

  final ProjectResources resources;
  List<ProjectResourceFeature> get features => resources.features;

  static List<ProjectResourceFeature> of(BuildContext context) =>
      context
          .dependOnInheritedWidgetOfExactType<ProjectResourcesScope>()
          ?.features ??
      const [];

  static OpenProjectResourceLink? linkHandler(
    BuildContext context, {
    String? kind,
    Uri? uri,
  }) => context
      .getInheritedWidgetOfExactType<ProjectResourcesScope>()
      ?.resources
      ._handler(kind, uri?.scheme);

  @override
  bool updateShouldNotify(ProjectResourcesScope oldWidget) =>
      resources != oldWidget.resources;
}
