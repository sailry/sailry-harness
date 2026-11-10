import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/identity_avatar.dart';
import 'choice_row.dart';
import 'team.dart';

/// A team's appearance; the Default team and plain teams share one icon.
class TeamIcon extends StatelessWidget {
  const TeamIcon({super.key, required this.team, this.size = 24});
  final Map<String, dynamic>? team;
  final double size;

  @override
  Widget build(BuildContext context) =>
      IdentityAvatar(identity: team, size: size, fallback: 'users');
}

String teamName(
  String execution,
  Map<String, dynamic>? team,
  String Function(String) tr,
) => execution == 'model'
    ? tr('executionModel')
    : team == null
    ? tr('teamDefault')
    : text(team['name'], tr('team'));

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
                    if (state.execution == 'model')
                      const IdentityAvatar(fallback: 'bot', size: 24)
                    else
                      TeamIcon(team: team),
                    Flexible(
                      child: Text(
                        teamName(state.execution, team, context.tr),
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
                  identity: null,
                  fallback: 'crown',
                  selected: member == null,
                  dot: false,
                  onTap: () => onMember(null),
                ),
                for (final role in state.members)
                  _avatar(
                    context,
                    key: ValueKey('team-member-${role['id']}'),
                    name: text(role['name']),
                    identity: {'appearance': state.appearance(role)},
                    fallback: 'bot',
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
    required Map<String, dynamic>? identity,
    required String fallback,
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
          child: Center(
            child: SizedBox.square(
              dimension: 40,
              child: Material(
                color: Colors.transparent,
                shape: const CircleBorder(),
                clipBehavior: Clip.antiAlias,
                child: InkWell(
                  key: key,
                  onTap: onTap,
                  customBorder: const CircleBorder(),
                  child: Stack(
                    children: [
                      Center(
                        child: IdentityAvatar(
                          identity: identity,
                          fallback: fallback,
                          selected: selected,
                        ),
                      ),
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
      ),
    );
  }
}

/// The Default team and stored teams; choosing one returns to the controls.
class TeamList extends StatelessWidget {
  const TeamList({
    super.key,
    required this.state,
    required this.onBack,
    required this.onSelected,
  });

  final SessionTeam state;
  final VoidCallback onBack;

  /// Receives the stored team, or null for the Default team.
  final ValueChanged<Map<String, dynamic>?> onSelected;

  @override
  Widget build(BuildContext context) {
    final current = state.team?['id'];
    Widget row(Map<String, dynamic>? team) => ChoiceRow(
      key: ValueKey('team-${team?['id'] ?? 'default'}'),
      leading: TeamIcon(team: team, size: 28),
      label: teamName('team', team, context.tr),
      selected: state.execution == 'team' && team?['id'] == current,
      onTap: () => onSelected(team),
    );
    return Column(
      mainAxisSize: MainAxisSize.min,
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
        row(null),
        for (final team in state.teams) row(team),
      ],
    );
  }
}
