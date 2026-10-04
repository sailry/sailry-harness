import 'dart:ui';
import 'package:flutter/material.dart';

enum SurfaceKind { card, navigation, sheet }

enum StatusTone { neutral, success, danger, warning, running }

enum AccentTone { blue, green, yellow, purple }

abstract final class SailryTheme {
  static final sheetBackdrop = ImageFilter.blur(sigmaX: 5, sigmaY: 5);

  static Color accentColor(BuildContext context, AccentTone tone) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    final (light, night) = switch (tone) {
      AccentTone.blue => (0xff2563eb, 0xff60a5fa),
      AccentTone.green => (0xff15803d, 0xff4ade80),
      AccentTone.yellow => (0xffa16207, 0xfffacc15),
      AccentTone.purple => (0xff9333ea, 0xffc084fc),
    };
    return Color(dark ? night : light);
  }

  static Color statusColor(BuildContext context, StatusTone tone) {
    final colors = Theme.of(context).colorScheme;
    return switch (tone) {
      StatusTone.success => colors.tertiary,
      StatusTone.danger => colors.error,
      StatusTone.warning => colors.secondary,
      StatusTone.neutral => colors.onSurfaceVariant,
      StatusTone.running => accentColor(context, AccentTone.blue),
    };
  }

  static ThemeData of(Brightness brightness) {
    final dark = brightness == Brightness.dark;
    final ink = Color(dark ? 0xffeeece4 : 0xff282722);
    final background = Color(dark ? 0xff101112 : 0xffeeebe6);
    final colors =
        ColorScheme.fromSeed(
          seedColor: const Color(0xff8c9873),
          brightness: brightness,
        ).copyWith(
          primary: Color(dark ? 0xffebe8dc : 0xff292925),
          onPrimary: Color(dark ? 0xff24251f : 0xfff5f2ec),
          primaryContainer: Color(dark ? 0x19b9bfa5 : 0x8cd5d2c9),
          onPrimaryContainer: ink,
          secondary: Color(dark ? 0xffdf9c70 : 0xffbb5b34),
          secondaryContainer: Color(dark ? 0x19b9bfa5 : 0x8cd5d2c9),
          onSecondaryContainer: ink,
          tertiary: Color(dark ? 0xffa4c886 : 0xff4a794e),
          surface: background,
          surfaceContainerLow: Color(dark ? 0x0cd4d9cc : 0x59dfddd7),
          surfaceContainer: Color(dark ? 0x0bc6cbbd : 0x5cffffff),
          surfaceContainerHigh: Color(dark ? 0x19b9bfa5 : 0x8cd5d2c9),
          surfaceContainerHighest: Color(dark ? 0x19b9bfa5 : 0x8cd5d2c9),
          surfaceTint: Colors.transparent,
          onSurface: ink,
          onSurfaceVariant: Color(dark ? 0xff9d9f93 : 0xff77746b),
          outline: Color(dark ? 0x11e6eed3 : 0x69ffffff),
          outlineVariant: Color(dark ? 0x14d1ddbf : 0x16494637),
        );
    final base = ThemeData(useMaterial3: true, colorScheme: colors);
    final feedbackTone = dark ? Colors.white : Colors.black;
    final pressOpacity = dark ? .045 : .035;
    final hoverOpacity = dark ? .025 : .02;
    final focusOpacity = dark ? .12 : .08;
    WidgetStateProperty<Color?> feedback(Color tone) =>
        WidgetStateProperty.resolveWith((states) {
          if (states.contains(WidgetState.disabled)) return Colors.transparent;
          if (states.contains(WidgetState.pressed)) {
            return tone.withValues(alpha: pressOpacity);
          }
          if (states.contains(WidgetState.focused)) {
            return tone.withValues(alpha: focusOpacity);
          }
          if (states.contains(WidgetState.hovered)) {
            return tone.withValues(alpha: hoverOpacity);
          }
          return null;
        });
    final overlay = feedback(feedbackTone);
    return base.copyWith(
      scaffoldBackgroundColor: background,
      splashFactory: NoSplash.splashFactory,
      splashColor: feedbackTone.withValues(alpha: pressOpacity),
      highlightColor: feedbackTone.withValues(alpha: pressOpacity),
      hoverColor: feedbackTone.withValues(alpha: hoverOpacity),
      focusColor: feedbackTone.withValues(alpha: focusOpacity),
      textTheme: base.textTheme.copyWith(
        bodyMedium: TextStyle(fontSize: 14, height: 1.55, color: ink),
        bodyLarge: TextStyle(fontSize: 14, height: 1.55, color: ink),
        bodySmall: TextStyle(fontSize: 14, height: 1.55, color: ink),
        labelSmall: TextStyle(fontSize: 14, height: 1.2, color: ink),
        labelMedium: TextStyle(fontSize: 14, height: 1.2, color: ink),
        labelLarge: TextStyle(fontSize: 14, height: 1.2, color: ink),
        titleLarge: TextStyle(
          fontSize: 22,
          height: 1.3,
          fontWeight: FontWeight.w600,
          color: ink,
        ),
        titleMedium: TextStyle(
          fontSize: 16,
          height: 1.5,
          fontWeight: FontWeight.w600,
          color: ink,
        ),
      ),
      iconTheme: IconThemeData(color: colors.onSurfaceVariant, size: 20),
      dividerTheme: DividerThemeData(
        color: colors.outlineVariant,
        space: 1,
        thickness: 1,
      ),
      dialogTheme: DialogThemeData(
        // Dialogs need their own tint as well as the blur supplied by showAppDialog.
        backgroundColor: Color(dark ? 0xb3181a1c : 0xd1ecebe5),
        elevation: 0,
        surfaceTintColor: Colors.transparent,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(28),
          side: BorderSide(color: colors.outlineVariant),
        ),
      ),
      cardTheme: CardThemeData(
        elevation: 0,
        margin: EdgeInsets.zero,
        color: colors.surfaceContainer,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(20),
          side: BorderSide(color: colors.outlineVariant),
        ),
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: colors.surfaceContainer,
        contentPadding: const EdgeInsets.symmetric(
          horizontal: 14,
          vertical: 12,
        ),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(12),
          borderSide: BorderSide(color: colors.outline),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(12),
          borderSide: BorderSide(color: colors.outline),
        ),
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          minimumSize: const Size(36, 36),
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(12),
          ),
        ).copyWith(overlayColor: feedback(dark ? Colors.black : Colors.white)),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          foregroundColor: ink,
          side: BorderSide(color: colors.outline),
          minimumSize: const Size(36, 36),
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(12),
          ),
        ).copyWith(overlayColor: overlay),
      ),
      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          foregroundColor: ink,
        ).copyWith(overlayColor: overlay),
      ),
      iconButtonTheme: IconButtonThemeData(
        style: ButtonStyle(overlayColor: overlay),
      ),
      segmentedButtonTheme: SegmentedButtonThemeData(
        style: ButtonStyle(overlayColor: overlay),
      ),
      listTileTheme: ListTileThemeData(
        contentPadding: const EdgeInsets.symmetric(horizontal: 14),
        minTileHeight: 48,
        iconColor: colors.onSurfaceVariant,
        titleTextStyle: TextStyle(fontSize: 14, color: ink),
        subtitleTextStyle: TextStyle(
          fontSize: 14,
          color: colors.onSurfaceVariant,
        ),
      ),
      bottomSheetTheme: BottomSheetThemeData(
        backgroundColor: Colors.transparent,
        modalBackgroundColor: Colors.transparent,
        elevation: 0,
        showDragHandle: false,
        dragHandleColor: colors.onSurfaceVariant.withValues(alpha: .35),
        shape: const RoundedRectangleBorder(
          borderRadius: BorderRadius.vertical(top: Radius.circular(28)),
        ),
      ),
      chipTheme: base.chipTheme.copyWith(
        side: BorderSide.none,
        backgroundColor: colors.surfaceContainer,
        selectedColor: colors.surfaceContainerHigh,
        labelStyle: TextStyle(fontSize: 14, color: ink),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(12)),
      ),
      tooltipTheme: TooltipThemeData(
        textStyle: TextStyle(fontSize: 14, color: colors.surface),
      ),
      expansionTileTheme: const ExpansionTileThemeData(
        shape: Border(),
        collapsedShape: Border(),
      ),
    );
  }

  // Glass materials follow the mobile design and its accepted refinements.
  static LinearGradient glassFill(BuildContext context, SurfaceKind kind) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    final tint = Theme.of(context).colorScheme.surfaceContainer;
    return LinearGradient(
      begin: Alignment.topLeft,
      end: Alignment.bottomRight,
      colors: switch (kind) {
        SurfaceKind.card || SurfaceKind.navigation =>
          dark
              ? const [Color(0x09ffffff), Color(0x08ffffff)]
              : [Color.alphaBlend(const Color(0x06ffffff), tint), tint],
        SurfaceKind.sheet =>
          dark
              ? const [Color(0xb3181a1c), Color(0xb816181a)]
              : const [Color(0xd1ecebe5), Color(0xd6e9e8e1)],
      },
      stops: const [0, 1],
    );
  }

  static LinearGradient glassEdge(BuildContext context, SurfaceKind kind) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    return LinearGradient(
      begin: Alignment.topCenter,
      end: Alignment.bottomCenter,
      colors: switch (kind) {
        SurfaceKind.card || SurfaceKind.navigation =>
          dark
              ? const [Color(0x1effffff), Color(0x14ffffff), Color(0x0cffffff)]
              : const [Color(0x50ffffff), Color(0x44ffffff), Color(0x38ffffff)],
        SurfaceKind.sheet =>
          dark
              ? const [Color(0x38ffffff), Color(0x24ffffff), Color(0x18ffffff)]
              : const [Color(0xb0ffffff), Color(0x98ffffff), Color(0x80ffffff)],
      },
    );
  }

  static ImageFilter glassFilter(BuildContext context, SurfaceKind kind) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    final (blur, saturation) = switch (kind) {
      // Keep card blur subtle while overlays diffuse the content behind them.
      SurfaceKind.card => (dark ? 4.0 : 18.0, dark ? 1.0 : 1.15),
      SurfaceKind.navigation => (28.0, dark ? 1.0 : 1.35),
      SurfaceKind.sheet => (22.0, dark ? 1.0 : 1.15),
    };
    final red = .213 * (1 - saturation);
    final green = .715 * (1 - saturation);
    final blue = .072 * (1 - saturation);
    return ImageFilter.compose(
      outer: ColorFilter.matrix([
        red + saturation,
        green,
        blue,
        0,
        0,
        red,
        green + saturation,
        blue,
        0,
        0,
        red,
        green,
        blue + saturation,
        0,
        0,
        0,
        0,
        0,
        1,
        0,
      ]),
      inner: ImageFilter.blur(sigmaX: blur, sigmaY: blur),
    );
  }

  static Color navigationSelection(BuildContext context) =>
      Theme.of(context).brightness == Brightness.dark
      ? const Color(0x12ffffff)
      : const Color(0x60ffffff);

  static Color userMessageFill(BuildContext context) =>
      Theme.of(context).brightness == Brightness.dark
      ? const Color(0x14ffffff)
      : const Color(0x8cd5d2c9);

  static List<BoxShadow> glassShadow(BuildContext context, SurfaceKind kind) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    return [
      switch (kind) {
        SurfaceKind.card => BoxShadow(
          color: Color(dark ? 0x05000000 : 0x04514934),
          offset: Offset(0, dark ? 2 : 3),
          blurRadius: dark ? 5 : 9,
        ),
        SurfaceKind.navigation => BoxShadow(
          color: Color(dark ? 0x18000000 : 0x1028271b),
          offset: Offset(0, dark ? 6 : 8),
          blurRadius: dark ? 23 : 25,
        ),
        SurfaceKind.sheet => const BoxShadow(
          color: Color(0x20000000),
          offset: Offset(0, -8),
          blurRadius: 35,
        ),
      },
    ];
  }
}
