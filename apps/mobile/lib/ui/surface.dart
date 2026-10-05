import 'package:flutter/material.dart';

import 'theme.dart';

class Surface extends StatelessWidget {
  const Surface({
    super.key,
    required this.child,
    this.padding = const EdgeInsets.all(16),
    this.radius = 20,
    this.onTap,
    this.kind = SurfaceKind.card,
  });
  final Widget child;
  final EdgeInsetsGeometry padding;
  final double radius;
  final VoidCallback? onTap;
  final SurfaceKind kind;
  @override
  Widget build(BuildContext context) {
    final borderRadius = BorderRadius.circular(radius);
    return DecoratedBox(
      decoration: BoxDecoration(
        borderRadius: borderRadius,
        boxShadow: SailryTheme.glassShadow(context, kind),
      ),
      child: ClipRRect(
        borderRadius: borderRadius,
        child: BackdropFilter(
          filter: SailryTheme.glassFilter(context, kind),
          child: Stack(
            fit: StackFit.passthrough,
            children: [
              Material(
                type: MaterialType.transparency,
                child: Ink(
                  decoration: BoxDecoration(
                    gradient: SailryTheme.glassFill(context, kind),
                  ),
                  child: InkWell(
                    onTap: onTap,
                    child: Padding(padding: padding, child: child),
                  ),
                ),
              ),
              Positioned.fill(
                child: IgnorePointer(
                  child: ShaderMask(
                    blendMode: BlendMode.srcIn,
                    shaderCallback: SailryTheme.glassEdge(
                      context,
                      kind,
                    ).createShader,
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        borderRadius: borderRadius,
                        border: Border.all(color: Colors.white),
                      ),
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
