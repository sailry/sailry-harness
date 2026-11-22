import '../features/resources/ports_contribution.dart';
import '../features/terminal/project_resource.dart';
import '../shell/assembly.dart';
import 'pages.dart';

final leanAssembly = MobileAssembly(
  pages: harnessPages,
  defaultRoute: chatRoute,
  resources: const [terminalResource, portsResource],
);
