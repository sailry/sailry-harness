import 'package:flutter/material.dart';

import '../../l10n/strings.dart';
import '../../ui/icons.dart';
import '../../ui/theme.dart';

/// The desktop welcome prompts only fill the local composer draft.
class ConversationWelcome extends StatelessWidget {
  const ConversationWelcome({super.key, required this.onPrompt});

  final ValueChanged<String>? onPrompt;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return LayoutBuilder(
      builder: (context, viewport) => SingleChildScrollView(
        child: ConstrainedBox(
          constraints: BoxConstraints(minHeight: viewport.maxHeight),
          child: Padding(
            padding: const EdgeInsets.all(20),
            child: Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 520),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      tr('brand'),
                      style: theme.textTheme.headlineLarge?.copyWith(
                        fontSize: 40,
                        fontWeight: FontWeight.w600,
                        letterSpacing: -1.4,
                      ),
                    ),
                    const SizedBox(height: 8),
                    Text(
                      tr('welcomeTitle'),
                      textAlign: TextAlign.center,
                      style: theme.textTheme.bodyMedium?.copyWith(
                        color: theme.colorScheme.onSurfaceVariant,
                      ),
                    ),
                    const SizedBox(height: 28),
                    LayoutBuilder(
                      builder: (context, constraints) {
                        final cards = [
                          for (final (name, icon, tone) in const [
                            ('Explore', 'folder', AccentTone.blue),
                            ('Build', 'terminal', AccentTone.green),
                            ('Review', 'eye', AccentTone.yellow),
                            ('Plan', 'grid', AccentTone.purple),
                          ])
                            _PromptCard(
                              name: name,
                              icon: icon,
                              tone: tone,
                              onPressed: onPrompt == null
                                  ? null
                                  : () => onPrompt!(tr('welcome${name}Prompt')),
                            ),
                        ];
                        final paired =
                            constraints.maxWidth >=
                            MediaQuery.textScalerOf(context).scale(128) * 2 +
                                12;
                        return Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          spacing: 12,
                          children: paired
                              ? [
                                  for (
                                    var index = 0;
                                    index < cards.length;
                                    index += 2
                                  )
                                    IntrinsicHeight(
                                      child: Row(
                                        crossAxisAlignment:
                                            CrossAxisAlignment.stretch,
                                        spacing: 12,
                                        children: [
                                          Expanded(child: cards[index]),
                                          Expanded(child: cards[index + 1]),
                                        ],
                                      ),
                                    ),
                                ]
                              : cards,
                        );
                      },
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _PromptCard extends StatelessWidget {
  const _PromptCard({
    required this.name,
    required this.icon,
    required this.tone,
    required this.onPressed,
  });

  final String name;
  final String icon;
  final AccentTone tone;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final accent = SailryTheme.accentColor(context, tone);
    return FilledButton(
      key: ValueKey('welcome-$name'),
      style: FilledButton.styleFrom(
        foregroundColor: theme.colorScheme.onSurface,
        backgroundColor: accent.withValues(
          alpha: theme.brightness == Brightness.dark ? .16 : .09,
        ),
        padding: const EdgeInsets.all(14),
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
      ),
      onPressed: onPressed,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              AppIcon(icon, size: 22, color: accent),
              AppIcon(
                'arrow-right',
                size: 16,
                color: accent.withValues(alpha: .6),
              ),
            ],
          ),
          const SizedBox(height: 16),
          Text(
            tr('welcome$name'),
            style: theme.textTheme.labelLarge?.copyWith(
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 4),
          Text(
            tr('welcome${name}Detail'),
            style: theme.textTheme.bodySmall?.copyWith(
              fontSize: 12,
              height: 1.4,
              color: theme.colorScheme.onSurfaceVariant,
            ),
          ),
        ],
      ),
    );
  }
}
