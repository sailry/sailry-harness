import '../../../runtime/json.dart';

/// Short Git status letter shown on change rows.
enum ChangeStatus {
  modified('M'),
  added('A'),
  deleted('D'),
  renamed('R'),
  untracked('?'),
  conflicted('C');

  const ChangeStatus(this.letter);
  final String letter;
}

typedef Entry = Map<String, dynamic>;
typedef LineStats = ({int additions, int deletions});

/// Status of [entry] on the staged or the working-tree side.
ChangeStatus changeStatus(Entry entry, {required bool staged}) {
  if (entry['conflicted'] == true) return ChangeStatus.conflicted;
  if (!staged && entry['untracked'] == true) return ChangeStatus.untracked;
  return switch (entry[staged ? 'staged' : 'unstaged']) {
    'added' => ChangeStatus.added,
    'deleted' => ChangeStatus.deleted,
    'renamed' => ChangeStatus.renamed,
    _ => ChangeStatus.modified,
  };
}

/// Splits status entries into working-tree and index groups; an entry with
/// both sides appears in each.
({List<Entry> unstaged, List<Entry> staged}) groupChanges(
  Iterable<Entry> entries,
) => (
  unstaged: [
    for (final entry in entries)
      if (entry['unstaged'] != null ||
          entry['untracked'] == true ||
          entry['conflicted'] == true)
        entry,
  ],
  staged: [
    for (final entry in entries)
      if (entry['staged'] != null) entry,
  ],
);

/// Line counts for one side of [entry], or both sides when [staged] is null.
LineStats lineStats(Entry entry, {bool? staged}) {
  final diff = object(
    entry[switch (staged) {
      null => 'diff',
      true => 'staged_diff',
      false => 'unstaged_diff',
    }],
  );
  return (
    additions: number(diff['additions']).toInt(),
    deletions: number(diff['deletions']).toInt(),
  );
}

LineStats totalStats(Iterable<Entry> entries) {
  var additions = 0;
  var deletions = 0;
  for (final entry in entries) {
    final stats = lineStats(entry);
    additions += stats.additions;
    deletions += stats.deletions;
  }
  return (additions: additions, deletions: deletions);
}

/// Discard restores tracked files from HEAD; untracked files go to the host
/// trash instead, and files added since HEAD have nothing to restore.
enum DiscardKind { restore, trash, none }

DiscardKind discardKind(Entry entry) {
  if (entry['untracked'] == true) return DiscardKind.trash;
  if (entry['staged'] == 'added' || entry['unstaged'] == 'added') {
    return DiscardKind.none;
  }
  return DiscardKind.restore;
}

/// Node commit options; unexposed flags keep their defaults.
Map<String, dynamic> commitOptions({
  bool skipHooks = false,
  bool push = false,
}) => {
  'signoff': false,
  'skip_hooks': skipHooks,
  'tracked': false,
  'all': false,
  'after': push ? 'push' : 'none',
  'push_remote': null,
};

/// Reference names arrive without `refs/`; drop the namespace for display.
String referenceLabel(String reference) {
  for (final prefix in ['heads/', 'remotes/', 'tags/']) {
    if (reference.startsWith(prefix)) return reference.substring(prefix.length);
  }
  return reference;
}

bool isCurrentReference(String reference, String? branch) =>
    branch != null && reference == 'heads/$branch';

String shortHash(String id) => id.length > 7 ? id.substring(0, 7) : id;

/// Local branches first, then remote branches, each in Node order.
({List<Entry> local, List<Entry> remote}) splitBranches(
  Iterable<Entry> branches,
) => (
  local: [
    for (final branch in branches)
      if (branch['remote'] != true) branch,
  ],
  remote: [
    for (final branch in branches)
      if (branch['remote'] == true) branch,
  ],
);
