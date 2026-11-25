import 'package:flutter/widgets.dart';

import '../features/resources/contributions.dart';
import '../features/updates/service.dart';
import '../runtime/preferences.dart';
import '../runtime/session.dart';

class MobileAssembly {
  MobileAssembly({
    required Iterable<MobilePage> pages,
    required this.defaultRoute,
    Iterable<ProjectResourceFeature> resources = const [],
    this.draftWorkspace,
    this.conversationConfiguration,
    this.createUpdates,
  }) : pages = List.unmodifiable(pages),
       projectResources = ProjectResources(resources) {
    final ids = this.pages.map((page) => page.id).toSet();
    if (ids.length != this.pages.length || ids.contains('')) {
      throw ArgumentError('Mobile page IDs must be nonempty and unique');
    }
    if (!ids.contains(defaultRoute)) {
      throw ArgumentError.value(defaultRoute, 'defaultRoute', 'Not a page ID');
    }
  }

  final List<MobilePage> pages;
  final String defaultRoute;
  final ProjectResources projectResources;
  List<ProjectResourceFeature> get resources => projectResources.features;
  final Widget Function(DraftWorkspaceContext)? draftWorkspace;
  final Widget Function(ConversationConfigurationContext)?
  conversationConfiguration;
  final AppUpdates Function()? createUpdates;

  MobilePage? page(String? id) =>
      pages.where((page) => page.id == id).firstOrNull;

  bool hasPage(String? id) => page(id) != null;

  String resolveRoute(String? saved) => page(saved)?.id ?? defaultRoute;
}

class ConversationConfigurationContext {
  const ConversationConfigurationContext({
    required this.host,
    required this.config,
    required this.enabled,
    required this.onChanged,
  });

  final HostConnection host;
  final Map<String, dynamic> config;
  final bool enabled;
  final ValueChanged<Map<String, dynamic>> onChanged;
}

class DraftWorkspaceContext {
  const DraftWorkspaceContext({
    required this.host,
    required this.worktrees,
    required this.selected,
    required this.enabled,
    required this.onChanged,
  });

  final HostConnection host;
  final List<Map<String, dynamic>> worktrees;
  final Map<String, dynamic>? selected;
  final bool enabled;
  final ValueChanged<Map<String, dynamic>> onChanged;
}

class MobileAssemblyScope extends InheritedWidget {
  const MobileAssemblyScope({
    super.key,
    required this.assembly,
    required super.child,
  });

  final MobileAssembly assembly;

  static MobileAssembly? maybeOf(BuildContext context) => context
      .dependOnInheritedWidgetOfExactType<MobileAssemblyScope>()
      ?.assembly;

  @override
  bool updateShouldNotify(MobileAssemblyScope oldWidget) =>
      assembly != oldWidget.assembly;
}

class MobilePage {
  const MobilePage({
    required this.id,
    required this.icon,
    required this.label,
    required this.build,
  });

  final String id;
  final String icon;
  final String label;
  final Widget Function(MobilePageContext) build;
}

typedef OpenTab = void Function(String id, {Object? arguments});
typedef BuildPage = Widget? Function(String id, {Object? arguments});

class MobilePageContext {
  const MobilePageContext({
    required this.preferences,
    required this.updates,
    required this.hasPage,
    required this.openTab,
    required this.buildPage,
    this.arguments,
    this.launchKey,
  });

  final AppPreferences preferences;
  final AppUpdates? updates;
  final bool Function(String) hasPage;
  final OpenTab openTab;
  final BuildPage buildPage;
  final Object? arguments;
  final Key? launchKey;
}
