import '../features/resources/files/contribution.dart';
import '../features/resources/git/contribution.dart';
import '../features/resources/ports_contribution.dart';
import '../features/terminal/project_resource.dart';
import '../shell/assembly.dart';
import 'pages.dart';

final harnessAssembly = MobileAssembly(
  pages: harnessPages,
  defaultRoute: chatRoute,
  resources: const [
    gitResource,
    filesResource,
    terminalResource,
    portsResource,
  ],
);
