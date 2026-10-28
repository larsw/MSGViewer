# MSG Viewer

A read-only GNOME viewer for Microsoft Outlook `.msg` files, for people on Linux who receive them as attachments.

## Language

### Messages

**Message**:
One email (or other Outlook item) as stored in a single `.msg` file.
_Avoid_: Mail, email file, item

**Message class**:
Outlook's label for what kind of item a Message is, e.g. `IPM.Note` for an ordinary email or `IPM.Schedule.Meeting.Request` for a meeting request.
_Avoid_: Message type, item type

**Embedded message**:
A Message carried as an attachment inside another Message.
_Avoid_: Nested message, forwarded attachment

**Property**:
A single named value stored in a Message, such as its subject, sender or delivery time.
_Avoid_: Field, attribute

**Header block**:
The part of the view showing subject, sender, date and recipients.
_Avoid_: Envelope, headers pane

### Bodies

**Body**:
The readable content of a Message, stored as plain text, HTML, RTF, or several of these.
_Avoid_: Content, text

**Compressed RTF**:
The compressed form in which Outlook stores RTF bodies.
_Avoid_: RTF blob

**Encapsulated body**:
An HTML or plain-text body that Outlook has wrapped inside RTF; it can be recovered exactly.
_Avoid_: Embedded HTML

**Genuine RTF**:
An RTF body that was authored as RTF and has no Encapsulated body inside it.
_Avoid_: Native RTF, real RTF

**Remote content**:
Anything a Body references from the internet, such as images or stylesheets; never loaded unless the reader asks for it on that Message.
_Avoid_: External images, tracking pixels

### Attachments

**Attachment**:
A file or Embedded message carried by a Message.
_Avoid_: Enclosure, file

**Inline attachment**:
An Attachment that exists only to be shown inside an HTML Body, such as a logo or a pasted screenshot; it is not listed with the other Attachments.
_Avoid_: Embedded image, cid image

**Risky attachment**:
An Attachment whose type can run code when opened, such as an executable, script, shortcut or macro-enabled Office file.
_Avoid_: Dangerous file, unsafe attachment

### Reading

**Partial read**:
The outcome when some of a Message could be read and the rest was damaged or missing; what was read is still shown.
_Avoid_: Corrupt message, broken file

**Unreadable file**:
A file that cannot be read as a Message at all.
_Avoid_: Invalid message
