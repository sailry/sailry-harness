import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

import '../features/updates/service.dart';
import '../l10n/strings.dart';
import '../runtime/preferences.dart';
import '../runtime/session.dart';
import '../ui/kit.dart';
import 'assembly.dart';

class MobileShell extends StatefulWidget {
  const MobileShell({
    super.key,
    required this.assembly,
    required this.preferences,
    required this.updates,
  });

  final MobileAssembly assembly;
  final AppPreferences preferences;
  final AppUpdates? updates;

  @override
  State<MobileShell> createState() => _MobileShellState();
}

class _MobileShellState extends State<MobileShell> {
  final _launches = <String, ({Object arguments, Key key})>{};

  @override
  void didUpdateWidget(MobileShell oldWidget) {
    super.didUpdateWidget(oldWidget);
    _launches.removeWhere((id, _) => !widget.assembly.hasPage(id));
  }

  void _openTab(String id, {Object? arguments}) {
    if (!widget.assembly.hasPage(id)) return;
    if (arguments != null || _launches.containsKey(id)) {
      setState(() {
        if (arguments == null) {
          _launches.remove(id);
        } else {
          _launches[id] = (arguments: arguments, key: UniqueKey());
        }
      });
    }
    unawaited(widget.preferences.setRoute(id));
  }

  Widget? _buildPage(String id, {Object? arguments}) {
    final page = widget.assembly.page(id);
    return page == null ? null : _build(page, arguments: arguments);
  }

  Widget _build(MobilePage page, {Object? arguments, Key? launchKey}) =>
      page.build(
        MobilePageContext(
          preferences: widget.preferences,
          updates: widget.updates,
          hasPage: widget.assembly.hasPage,
          openTab: _openTab,
          buildPage: _buildPage,
          arguments: arguments,
          launchKey: launchKey,
        ),
      );

  @override
  Widget build(BuildContext context) {
    final assembly = widget.assembly;
    final preferences = widget.preferences;
    final session = AppSession.maybeOf(context);
    return ListenableBuilder(
      listenable: preferences,
      builder: (context, _) {
        final selected = assembly.resolveRoute(preferences.route);
        return Scaffold(
          extendBody: true,
          backgroundColor: Colors.transparent,
          body: SlidableAutoCloseBehavior(
            child: session != null && !session.ready && session.error != null
                ? PageFrame(
                    title: context.tr('brand'),
                    failure: FailureState(
                      message: context.tr('startupFailed'),
                      onRetry: session.start,
                    ),
                    child: const SizedBox.shrink(),
                  )
                : IndexedStack(
                    index: assembly.pages.indexWhere(
                      (page) => page.id == selected,
                    ),
                    children: [
                      for (final page in assembly.pages)
                        TickerMode(
                          key: ValueKey(page.id),
                          enabled: page.id == selected,
                          child: _build(
                            page,
                            arguments: _launches[page.id]?.arguments,
                            launchKey: _launches[page.id]?.key,
                          ),
                        ),
                    ],
                  ),
          ),
          bottomNavigationBar: FloatingNavigation(
            items: [
              for (final page in assembly.pages)
                (id: page.id, icon: page.icon, label: page.label),
            ],
            selected: selected,
            onSelected: _openTab,
          ),
        );
      },
    );
  }
}
