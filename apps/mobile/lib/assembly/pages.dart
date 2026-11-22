import '../features/conversations/tasks_page.dart';
import '../features/resources/hosts_page.dart';
import '../features/resources/projects_page.dart';
import '../features/settings/settings_page.dart';
import '../runtime/session.dart';
import '../shell/assembly.dart';

const chatRoute = 'chat';
const hostsRoute = 'hosts';
const projectsRoute = 'projects';
const settingsRoute = 'settings';

enum _TaskBrowse { sessions, terminals }

final harnessPages = List<MobilePage>.unmodifiable([
  MobilePage(
    id: chatRoute,
    icon: 'chat',
    label: 'chat',
    build: (page) => TasksPage(
      key: page.launchKey,
      initialFilter: page.arguments == _TaskBrowse.terminals
          ? 'terminal'
          : 'all',
      sessionsOnly: page.arguments == _TaskBrowse.sessions,
    ),
  ),
  MobilePage(
    id: hostsRoute,
    icon: 'server',
    label: 'hosts',
    build: (page) => HostsPage(
      onBrowseTasks: page.hasPage(chatRoute)
          ? (kind) => page.openTab(
              chatRoute,
              arguments: kind == 'terminals'
                  ? _TaskBrowse.terminals
                  : _TaskBrowse.sessions,
            )
          : null,
      projectList: page.hasPage(projectsRoute)
          ? (host) => page.buildPage(projectsRoute, arguments: host)
          : null,
    ),
  ),
  MobilePage(
    id: projectsRoute,
    icon: 'folder',
    label: 'projects',
    build: (page) => ProjectsPage(host: page.arguments as HostConnection?),
  ),
  MobilePage(
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
  ),
]);
