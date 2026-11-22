import 'package:flutter/widgets.dart';

import '../../runtime/session.dart';
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

class ProjectResourceFeature {
  const ProjectResourceFeature({
    this.entry,
    this.entryChanges,
    this.header,
    this.action,
    this.openDirectory,
  });

  final ProjectResourceEntry? entry;
  final Listenable Function(ProjectResourceView)? entryChanges;
  final ProjectResourceHeader? header;
  final ProjectResourceAction? action;
  final OpenDirectory? openDirectory;
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
