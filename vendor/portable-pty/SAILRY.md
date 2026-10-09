# Portable PTY patch notice

Source and licensing are recorded in
[Portable PTY patches](../../third_party_licenses/portable-pty.md).

Windows commands can retain a caller-owned Job through `CommandBuilder`.
The ConPTY process starts suspended, joins that Job and then resumes. Ordinary
builders retain the upstream startup behavior.

`ConPtySystem` can disable cursor inheritance for a command host that does not
answer terminal cursor queries. Its default retains the upstream flags. Managed
commands close their pseudoconsole on a blocking worker while the output reader
continues draining, including when execution is cancelled or dropped.
The original output reader drops before the pseudoconsole. Active cloned readers
still drain its final frame; failed startup without a reader breaks the output
pipe before closure so cleanup cannot wait for an absent reader.
