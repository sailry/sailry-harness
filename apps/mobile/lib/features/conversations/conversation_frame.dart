import 'package:flutter/material.dart';
import '../../ui/page_heading.dart';

/// Conversation content scrolls behind the floating controls and themed fade.
class ConversationFrame extends StatelessWidget {
  const ConversationFrame({
    super.key,
    required this.title,
    this.leading,
    required this.actions,
    required this.bodyBuilder,
    required this.composer,
  });

  final String title;
  final Widget? leading;
  final List<Widget> actions;
  final Widget Function(BuildContext context, double bottomInset) bodyBuilder;
  final Widget composer;

  // Reserve room at the start of history, not above the scrolling viewport.
  static double contentInset(BuildContext context) =>
      MediaQuery.paddingOf(context).top + 64;

  @override
  Widget build(BuildContext context) {
    final surface = Theme.of(context).colorScheme.surface;
    // Reserve the keyboard once for both overlays and history. Scaffold's
    // extended body then supplies the measured composer height as padding.
    return Padding(
      padding: EdgeInsets.only(bottom: MediaQuery.viewInsetsOf(context).bottom),
      child: MediaQuery.removeViewInsets(
        context: context,
        removeBottom: true,
        child: Scaffold(
          resizeToAvoidBottomInset: false,
          extendBody: true,
          backgroundColor: Colors.transparent,
          bottomNavigationBar: SafeArea(
            top: false,
            bottom: false,
            child: Stack(
              children: [
                Positioned.fill(
                  child: IgnorePointer(
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        gradient: LinearGradient(
                          begin: Alignment.topCenter,
                          end: Alignment.bottomCenter,
                          colors: [
                            surface.withValues(alpha: 0),
                            surface,
                            surface,
                          ],
                          stops: const [0, .65, 1],
                        ),
                      ),
                    ),
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: composer,
                ),
              ],
            ),
          ),
          body: Builder(
            builder: (context) => SafeArea(
              top: false,
              bottom: false,
              child: Stack(
                fit: StackFit.expand,
                children: [
                  bodyBuilder(context, MediaQuery.paddingOf(context).bottom),
                  Positioned(
                    top: 0,
                    left: 0,
                    right: 0,
                    height: contentInset(context),
                    child: IgnorePointer(
                      child: DecoratedBox(
                        decoration: BoxDecoration(
                          gradient: LinearGradient(
                            begin: Alignment.topCenter,
                            end: Alignment.bottomCenter,
                            colors: [
                              surface,
                              surface,
                              surface.withValues(alpha: 0),
                            ],
                            stops: const [0, .35, 1],
                          ),
                        ),
                      ),
                    ),
                  ),
                  Positioned(
                    top: 0,
                    left: 0,
                    right: 0,
                    child: SafeArea(
                      bottom: false,
                      child: Padding(
                        padding: const EdgeInsets.fromLTRB(20, 19, 20, 8),
                        child: Row(
                          children: [
                            if (ModalRoute.canPopOf(context) == true) ...[
                              Expanded(
                                child: PageHeading(title: title, size: 18),
                              ),
                              if (leading != null) ...[
                                const SizedBox(width: 4),
                                SizedBox(width: 36, child: leading),
                              ],
                              const SizedBox(width: 4),
                            ] else
                              Expanded(child: leading ?? const SizedBox()),
                            ...actions,
                          ],
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
