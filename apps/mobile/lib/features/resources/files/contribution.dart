import 'package:flutter/widgets.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../contributions.dart';
import '../files_page.dart';
import '../overview_cards.dart';

const filesResource = ProjectResourceFeature(
  entry: _filesEntry,
  openDirectory: _openDirectory,
);

ResourceEntry _filesEntry(BuildContext context, ProjectResourceView view) => (
  icon: 'folder',
  label: context.tr('files'),
  detail: null,
  onTap: () =>
      view.open(FilesPage(hostId: view.host.id, worktreeId: view.worktree)),
);

Future<void> _openDirectory(
  BuildContext context,
  HostConnection host,
  String worktree,
  String directory,
) => pushPage<void>(
  context,
  FilesPage(hostId: host.id, worktreeId: worktree, initialDirectory: directory),
);
