# Runtime dependency closure, excluding development and build-only edges.
def closure($edges):
  . as $seen
  | (. + [.[] as $id | $edges[$id][]?] | unique)
  | if . == $seen then . else closure($edges) end;

(.resolve.nodes | map({key: .id, value: [
    .deps[] | select(any(.dep_kinds[]; .kind == null)) | .pkg
  ]}) | from_entries) as $edges
| ([.packages[] | select(.name == $package) | .id] | closure($edges)) as $selected
| [.packages[] | select(.id as $id | $selected | index($id))]
| sort_by(.name, .version)
