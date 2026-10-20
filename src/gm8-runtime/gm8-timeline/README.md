# gm8-timeline

Bounded `no_std` parser for compressed GameMaker 8.0 Timeline records. It parses
version 500 timelines, version 400 moments, moment indices, and their action
records. Timeline actions reuse `gm8-object::parse_code_action`, the same
schema used for Object event actions.

The table loader validates the indexed GM8.0 directory and caps compressed and
decompressed bytes, names, moments, actions per timeline, timeline count, and
aggregate action count. It does not schedule moments or execute their actions.
