# Sample files

Every file here must be redistributable under terms compatible with Apache-2.0. Record where each came from and under what license.

| File | Source | License |
|---|---|---|
| `testMSG.msg` | Apache Tika, `tika-parsers/tika-parsers-standard/tika-parsers-standard-modules/tika-parser-microsoft-module/src/test/resources/test-documents/testMSG.msg` ([apache/tika](https://github.com/apache/tika), `main`, fetched 2026-10-28) | Apache-2.0 |
| `test-outlook.msg` | Apache Tika, same directory, `test-outlook.msg` (`main`, fetched 2026-10-28). 8-bit strings in ISO-8859-1. | Apache-2.0 |
| `testMSG_chinese.msg` | Apache Tika, same directory, `testMSG_chinese.msg` (`main`, fetched 2026-10-28). 8-bit strings in Big5 (codepage 950). | Apache-2.0 |
| `testMSG_dateHeaderOnly.msg` | Apache Tika, same directory, `testMSG_dateHeaderOnly.msg` (`main`, fetched 2026-10-28). Delivery time but no submit time. | Apache-2.0 |
| `testMSG_forwarded.msg` | Apache Tika, same directory, `testMSG_forwarded.msg` (`main`, fetched 2026-10-28). Exchange recipient with an SMTP address. | Apache-2.0 |
| `testMSG_att_doc.msg` | Apache Tika, same directory, `testMSG_att_doc.msg` (`main`, fetched 2026-10-28). Attachments with long and 8.3 names. | Apache-2.0 |
| `testMSG_att_msg.msg` | Apache Tika, same directory, `testMSG_att_msg.msg` (`main`, fetched 2026-10-28). An Embedded message and a PDF. | Apache-2.0 |
| `test-outlook2003.msg` | Apache Tika, same directory, `test-outlook2003.msg` (`main`, fetched 2026-10-28). Unicode plain-text Body; Inline attachments. | Apache-2.0 |
| `testMSG_Appointment.msg` | Apache Tika, same directory, `testMSG_Appointment.msg` (`main`, fetched 2026-10-28). Message class `IPM.Appointment`. | Apache-2.0 |
| `testMSG_Contact.msg` | Apache Tika, same directory, `testMSG_Contact.msg` (`main`, fetched 2026-10-28). Message class `IPM.Contact`; no recipients. | Apache-2.0 |
| `testMSG_Task.msg` | Apache Tika, same directory, `testMSG_Task.msg` (`main`, fetched 2026-10-28). Message class `IPM.Task`. | Apache-2.0 |
| `testMSG_StickyNote.msg` | Apache Tika, same directory, `testMSG_StickyNote.msg` (`main`, fetched 2026-10-28). Message class `IPM.StickyNote`; no sender. | Apache-2.0 |
| `testMSG_Post.msg` | Apache Tika, same directory, `testMSG_Post.msg` (`main`, fetched 2026-10-28). Message class `IPM.Post`. | Apache-2.0 |
| `attachment_msg_inlineImg.msg` | Apache POI, `test-data/hsmf/attachment_msg_inlineImg.msg` ([apache/poi](https://github.com/apache/poi), `trunk`, fetched 2026-10-28). Binary HTML Body declaring ISO-8859-1; Inline attachments referenced by `cid:` links. | Apache-2.0 |
| `HTMLBodyBinary_CP1251.msg` | Apache POI, same directory, `HTMLBodyBinary_CP1251.msg` (`trunk`, fetched 2026-10-28). Binary HTML Body with a malformed `<meta charset>`, in the Internet codepage (1251). | Apache-2.0 |
| `cyrillic_message.msg` | Apache POI, same directory, `cyrillic_message.msg` (`trunk`, fetched 2026-10-28). Compressed RTF encapsulating plain text (`\fromtext`), in codepage 1251. | Apache-2.0 |
| `message_1979.msg` | Apache POI, same directory, `message_1979.msg` (`trunk`, fetched 2026-10-28). Compressed RTF encapsulating HTML (`\fromhtml1`), in codepage 65001 (UTF-8). | Apache-2.0 |
| `quick.msg` | Apache POI, same directory, `quick.msg` (`trunk`, fetched 2026-10-28). Genuine RTF and a plain-text Body. | Apache-2.0 |

Apache Tika is licensed Apache-2.0, and its `LICENSE.txt` lists no other terms for these files (it does for some other test documents, such as `testDXF.dxf`).

Apache POI is licensed Apache-2.0, and its `legal/LICENSE` lists no other terms for its test data.

Edge cases (codepages, missing Properties, many recipients, signed and encrypted Messages) are covered by small files that `tests/generated.rs` builds at test time rather than by checked-in samples.

None of the Tika or POI samples stores its Body only as Compressed RTF, so `tests/rtf.rs` builds such Messages from the Compressed RTF of the samples above, and from RTF it compresses itself.

Real `.msg` files for local testing go in `/tests/local/` at the repository root, which git ignores. Never commit them.
