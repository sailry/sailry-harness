# Web search

An ordinary Agent Plugins package enabling the selected model's hosted web search.
The package owns the capability declaration and localized tool display. It needs
neither a desktop view nor a JavaScript process; there is no package-name dispatch
in the host. Other packages can declare the same public `model_tools` capability.

The execution Node adapts the capability to its configured provider, preserving
model compatibility, credentials, native results, citations and canonical history.
It does not substitute HTTP search when the selected model lacks hosted search.
Duplicate declarations across enabled packages register one provider capability.
Assistant packages declare their own capability and select it with
`{"kind":"model","capability":"web_search"}` in their assistant tool list.

New turns capture the enabled immutable package version. Updating, disabling or
removing the package does not alter admitted work or erase recorded tool labels.
Installation and removal use the same package manager as any external plugin.
