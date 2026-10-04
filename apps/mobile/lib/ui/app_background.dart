import 'package:flutter/material.dart';

/// An opaque themed backdrop for a route and its translucent surfaces.
class AppBackground extends StatelessWidget {
  const AppBackground({super.key, required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) => ColoredBox(
    color: Theme.of(context).colorScheme.surface,
    child: SizedBox.expand(child: child),
  );
}
