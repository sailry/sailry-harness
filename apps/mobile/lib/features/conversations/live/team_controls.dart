import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import 'choice_row.dart';
import 'team.dart';

/// A team's appearance; the Default team and plain teams share one icon.
class TeamIcon extends StatelessWidget {
  const TeamIcon({super.key, required this.team, this.size = 16});
  final Map<String, dynamic>? team;
  final double size;

  @override
  Widget build(BuildContext context) =>
      ProjectIcon(project: team ?? const {}, size: size, fallback: 'profile');
}

String teamName(Map<String, dynamic>? team, String Function(String) tr) =>
    team == null ? tr('teamDefault') : text(team['name'], tr('team'));

/// The selected team followed by the Lead and member avatars.
class TeamBar extends StatelessWidget {
  const TeamBar({
    super.key,
    required this.state,
    required this.member,
    required this.onTeam,
    required this.onMember,
  });

  final SessionTeam state;

  /// The selected member's role ID; null selects the Lead.
  final String? member;
  final VoidCallback onTeam;
  final ValueChanged<String?> onMember;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final team = state.team;
    return SizedBox(
      height: 44,
      child: Row(
        children: [
          InkWell(
            key: const ValueKey('team-choose'),
            borderRadius: BorderRadius.circular(10),
            onTap: onTeam,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 8),
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 150),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  spacing: 6,
                  children: [
                    TeamIcon(team: team),
                    Flexible(
                      child: Text(
                        teamName(team, context.tr),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(fontSize: 14, color: colors.onSurface),
                      ),
                    ),
                    AppIcon(
                      'chevron',
                      size: 12,
                      color: colors.onSurfaceVariant,
                    ),
                  ],
                ),
              ),
            ),
          ),
          const VerticalDivider(width: 16, indent: 12, endIndent: 12),
          Expanded(
            child: ListView(
              scrollDirection: Axis.horizontal,
              children: [
                _avatar(
                  context,
                  key: const ValueKey('team-lead'),
                  name: context.tr('teamLead'),
                  icon: const AppIcon('spark', size: 18),
                  selected: member == null,
                  dot: false,
                  onTap: () => onMember(null),
                ),
                for (final role in state.members)
                  _avatar(
                    context,
                    key: ValueKey('team-member-${role['id']}'),
                    name: text(role['name']),
                    icon: ProjectIcon(project: role, size: 18, fallback: 'ai'),
                    selected: member == role['id'],
                    dot: state.overridden(text(role['id'])),
                    onTap: () => onMember(text(role['id'])),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _avatar(
    BuildContext context, {
    required Key key,
    required String name,
    required Widget icon,
    required bool selected,
    required bool dot,
    required VoidCallback onTap,
  }) {
    final colors = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(right: 4),
      child: Tooltip(
        message: name,
        child: Semantics(
          button: true,
          selected: selected,
          label: name,
          excludeSemantics: true,
          child: Material(
            color: selected ? colors.surfaceContainerHigh : Colors.transparent,
            borderRadius: BorderRadius.circular(10),
            clipBehavior: Clip.antiAlias,
            child: InkWell(
              key: key,
              onTap: onTap,
              child: SizedBox.square(
                dimension: 40,
                child: Stack(
                  children: [
                    Center(child: icon),
                    if (dot)
                      Positioned(
                        top: 6,
                        right: 6,
                        child: Container(
                          width: 7,
                          height: 7,
                          decoration: BoxDecoration(
                            shape: BoxShape.circle,
                            color: colors.primary,
                          ),
                        ),
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

/// The Default team and stored teams; choosing one returns to the models.
class TeamList extends StatelessWidget {
  const TeamList({
    super.key,
    required this.state,
    required this.onBack,
    required this.onSelected,
  });

  final SessionTeam state;
  final VoidCallback onBack;

  /// Receives a stored team, or null for the Default team.
  final ValueChanged<Map<String, dynamic>?> onSelected;

  @override
  Widget build(BuildContext context) {
    final current = state.team?['id'];
    Widget row(Map<String, dynamic>? team) => ChoiceRow(
      key: ValueKey('team-${team?['id'] ?? 'default'}'),
      leading: TeamIcon(team: team, size: 20),
      label: teamName(team, context.tr),
      selected: team?['id'] == current,
      onTap: () => onSelected(team),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            IconButton(
              tooltip: context.tr('back'),
              onPressed: onBack,
              icon: const AppIcon('back', size: 18),
            ),
            Expanded(
              child: Text(
                context.tr('team'),
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.titleMedium,
              ),
            ),
            const SizedBox(width: 48),
          ],
        ),
        Expanded(
          child: ListView(
            padding: const EdgeInsets.only(bottom: 8),
            children: [row(null), for (final team in state.teams) row(team)],
          ),
        ),
      ],
    );
  }
}
