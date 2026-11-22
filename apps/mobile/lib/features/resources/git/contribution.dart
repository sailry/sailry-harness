import 'dart:async';

import 'package:flutter/widgets.dart';

import '../contributions.dart';
import '../git_page.dart';
import 'overview_card.dart';
import 'summary.dart';

const gitResource = ProjectResourceFeature(header: _header);

Widget _header(ProjectResourceView view, Widget Function(String?) heading) =>
    _GitOverview(view: view, heading: heading);

class _GitOverview extends StatefulWidget {
  const _GitOverview({required this.view, required this.heading});

  final ProjectResourceView view;
  final Widget Function(String?) heading;

  @override
  State<_GitOverview> createState() => _GitOverviewState();
}

class _GitOverviewState extends State<_GitOverview> {
  final _summary = GitSummary();

  @override
  void initState() {
    super.initState();
    _summary.track(widget.view.host, widget.view.worktree);
  }

  @override
  void didUpdateWidget(_GitOverview oldWidget) {
    super.didUpdateWidget(oldWidget);
    final view = widget.view;
    final old = oldWidget.view;
    if (view.host != old.host || view.worktree != old.worktree) {
      _summary.track(view.host, view.worktree);
    } else if (view.revision != old.revision) {
      unawaited(_summary.load());
    }
  }

  @override
  void dispose() {
    _summary.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: _summary,
    builder: (context, _) => Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        widget.heading(_summary.status?['branch'] as String?),
        GitOverviewCard(
          summary: _summary,
          onOpen: () {
            final view = widget.view;
            unawaited(
              view.open(
                GitPage(hostId: view.host.id, worktreeId: view.worktree),
              ),
            );
          },
        ),
      ],
    ),
  );
}
