import '../features/conversations/tasks_page.dart';
import '../features/resources/contributions.dart';
import '../features/resources/hosts_page.dart';
import '../features/resources/projects_page.dart';
import '../features/settings/settings_page.dart';
import '../runtime/session.dart';
import '../shell/assembly.dart';

const chatRoute = 'chat';
const hostsRoute = 'hosts';
const projectsRoute = 'projects';
const settingsRoute = 'settings';

/// Task list; a [TaskBrowse] argument selects its opening view.
final tasksPage = MobilePage(
  id: chatRoute,
  icon: 'chat',
  label: 'chat',
  build: (page) {
    final browse = page.arguments as TaskBrowse?;
    return TasksPage(
      key: page.launchKey,
      initialFilter: browse?.filter ?? 'all',
      sessionsOnly: browse?.sessionsOnly ?? false,
    );
  },
);

/// Host overview; its counts open the task and project pages when assembled.
final hostsPage = MobilePage(
  id: hostsRoute,
  icon: 'server',
  label: 'hosts',
  build: (page) => HostsPage(
    onBrowseTasks: page.hasPage(chatRoute)
        ? (browse) => page.openTab(chatRoute, arguments: browse)
        : null,
    projectList: page.hasPage(projectsRoute)
        ? (host) => page.buildPage(projectsRoute, arguments: host)
        : null,
  ),
);

final projectsPage = MobilePage(
  id: projectsRoute,
  icon: 'folder',
  label: 'projects',
  build: (page) => ProjectsPage(host: page.arguments as HostConnection?),
);

final settingsPage = MobilePage(
  id: settingsRoute,
  icon: 'settings',
  label: 'settings',
  build: (page) {
    final preferences = page.preferences;
    return SettingsPage(
      updates: page.updates,
      language: preferences.language,
      onLanguageChanged: preferences.setLanguage,
      themeMode: preferences.mode,
      onThemeChanged: preferences.setTheme,
      brand: preferences.brand,
      onBrandChanged: preferences.setBrand,
      notifications: preferences.notifications,
      onNotificationsChanged: preferences.setNotifications,
    );
  },
);

/// The default root pages in navigation order.
final harnessPages = List<MobilePage>.unmodifiable([
  tasksPage,
  hostsPage,
  projectsPage,
  settingsPage,
]);
