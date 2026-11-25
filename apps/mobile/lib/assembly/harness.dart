import '../features/resources/ports_contribution.dart';
import '../shell/assembly.dart';
import 'pages.dart';

final harnessAssembly = MobileAssembly(
  pages: harnessPages,
  defaultRoute: chatRoute,
  resources: const [portsResource],
);
