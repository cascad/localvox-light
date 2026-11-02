# rs-morphy4 0.1.3

LocalVox uses [rs-morphy4](https://github.com/LdDl/rs-morphy4), pinned to version
0.1.3 in Cargo.lock (crate SHA-256
`6ea0c73b6b0b73c028e4e194b7a5657c4e26937e97e97d04d026b09233a0bd3b`).

The Rust implementation is by Lopanov Dmitrii (LdDl), Apache-2.0, with portions
under MIT. [NOTICE](NOTICE), [Apache license](LICENSE), [MIT notices](LICENSE-MIT)
and [DAWG MIT notice](LICENSE-MIT-DAWG) are copied unchanged from that crate.

The embedded Russian dictionary originates from OpenCorpora contributors,
distributed by the MAWO Team's mawo-pymorphy3, commit
`f60ff89ced0aab256470061e1e566fab317b6170` (dictionary revision 417260).
The data is used unchanged under [CC BY-SA 3.0](LICENSE-CC-BY-SA-3.0.txt).
Sources: [OpenCorpora](https://opencorpora.org/),
[pinned dictionary distribution](https://github.com/mawo-ru/mawo-pymorphy3/tree/f60ff89ced0aab256470061e1e566fab317b6170/mawo_pymorphy3/dicts_ru).

Dictionary bytes are supplied by the Cargo dependency, not copied into this
directory. LocalVox uses dictionary-only lookup; suffix prediction and word/tag
frequency ranking are not used. The texts in this directory accompany release
archives containing the compiled dictionary. The implementation's license does
not replace the dictionary's attribution and ShareAlike terms.
