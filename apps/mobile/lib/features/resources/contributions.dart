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

/// The exact originating route, using the full Host connection ID.
typedef ProjectLinkScope = ({String host, String worktree, String session});

/// A target match takes precedence over a scheme match. Within a route the
/// highest applicable priority owns the open, including any failure.
/// File and directory targets carry a Node-relative path in `data`.
class ProjectLinkContribution {
  const ProjectLinkContribution.target(
    String this.kind,
    this.open, {
    this.priority = 0,
    this.scope,
  }) : scheme = null;
  const ProjectLinkContribution.scheme(
    String this.scheme,
    this.open, {
    this.priority = 0,
    this.scope,
  }) : kind = null;

  final String? kind;
  final String? scheme;
  final OpenProjectResourceLink open;
  final int priority;

  /// Null is the assembly's global scope and overlaps every origin.
  final ProjectLinkScope? scope;
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

typedef _ProjectLinkRoute = ({String? kind, String? scheme});

class ProjectResources {
  static const _systemLinkPriority = -1;
  static const _systemSchemes = {'http', 'https', 'mailto', 'sailry'};

  static bool _hasSystemLink({String? kind, String? scheme}) => kind == null
      ? _systemSchemes.contains(scheme)
      : kind == 'file' || kind == 'attachment';

  ProjectResources(Iterable<ProjectResourceFeature> features)
    : features = List.unmodifiable(features) {
    final scopesByPriority =
        <(_ProjectLinkRoute, int), Set<ProjectLinkScope?>>{};
    void register(ProjectLinkContribution contribution) {
      final kind = contribution.kind;
      final scheme = contribution.scheme?.toLowerCase();
      final route = (kind: kind, scheme: scheme);
      final scope = contribution.scope;
      final priority = contribution.priority;
      final scopes = scopesByPriority.putIfAbsent(
        (route, priority),
        () => {
          if (priority == _systemLinkPriority &&
              _hasSystemLink(kind: kind, scheme: scheme))
            null,
        },
      );
      if ((scope == null && scopes.isNotEmpty) ||
          scopes.contains(null) ||
          !scopes.add(scope)) {
        final label = kind == null ? 'scheme/$scheme' : 'target/$kind';
        throw ArgumentError(
          'Duplicate project link route $label at priority $priority',
        );
      }
      final key = (route, scope);
      final current = _handlers[key];
      if (current == null || priority > current.priority) {
        _handlers[key] = contribution;
      }
    }

    for (final feature in this.features) {
      for (final link in feature.links) {
        register(link);
      }
      if (feature.openDirectory case final open?) {
        register(
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
      <(_ProjectLinkRoute, ProjectLinkScope?), ProjectLinkContribution>{};

  ProjectLinkContribution? _candidate(
    _ProjectLinkRoute route,
    ProjectLinkScope? scope,
  ) {
    final global = _handlers[(route, null)];
    final scoped = scope == null ? null : _handlers[(route, scope)];
    if (scoped != null &&
        (global == null || scoped.priority > global.priority)) {
      return scoped;
    }
    return global;
  }

  ProjectLinkContribution? _handler(
    String? kind,
    String? scheme,
    ProjectLinkScope? scope,
  ) {
    final candidate =
        (kind == null ? null : _candidate((kind: kind, scheme: null), scope)) ??
        (scheme == null || scheme.isEmpty
            ? null
            : _candidate((kind: null, scheme: scheme), scope));
    if (candidate != null &&
        (!_hasSystemLink(kind: kind, scheme: scheme) ||
            candidate.priority > _systemLinkPriority)) {
      return candidate;
    }
    return null;
  }
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
    ProjectLinkScope? scope,
  }) => resolveLink(context, kind: kind, uri: uri, scope: scope)?.open;

  static ProjectLinkContribution? resolveLink(
    BuildContext context, {
    String? kind,
    Uri? uri,
    ProjectLinkScope? scope,
  }) => context
      .getInheritedWidgetOfExactType<ProjectResourcesScope>()
      ?.resources
      ._handler(kind, uri?.scheme, scope);

  @override
  bool updateShouldNotify(ProjectResourcesScope oldWidget) =>
      resources != oldWidget.resources;
}
