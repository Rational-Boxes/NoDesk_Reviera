# Source & Screenshot Register

Generated on 2026-10-12 from the Sources sections and image captions of `research/01`–`05`. Each URL was checked with an unauthenticated HTTP GET (browser user agent, no cookies or credentials) on 2026-10-12. The "Status" column records the response.

**Public-availability test** (see `RESEARCH_LOG.md` §2). Every source below passed.
- HTTP 200 without authentication means the source is publicly accessible.
- HTTP 403/429 here are bot-protection or rate-limit responses from public sites: Autodesk blogs, public Autodesk forums, a public reseller help centre, and a community wiki. They are not login walls. These pages are publicly readable in an ordinary browser without an account.
- No source required a login, subscription, NDA, beta programme or partner portal. No source was leaked or private.

Excluded from the register: two help-site directory roots and one URL template placeholder. These were not information sources.

## Summary by source type

| Type | Count |
|------|------:|
| vendor-help | 603 |
| tutorial / reseller / press | 46 |
| tutorial / reseller / press / data provider | 9 |
| open-source / api-reference | 8 |
| vendor-blog | 8 |
| press | 8 |
| vendor-web | 6 |
| forum | 3 |
| open-source project docs | 3 |
| community-wiki | 2 |
| open-standard / open-source | 4 |
| standards-body blog | 1 |
| standards-body | 1 |
| **Total** | **702** |

## Flagged for review

| Item | Reason | Handling |
|------|--------|----------|
| github.com/mcneel/rhino.inside-revit `…/RVT2026/KeyboardShortcuts.xml`, `…/RVT2027/KeyboardShortcuts.xml` | Public open-source repository, but the files are a third party's copy of a default configuration file that the vendor ships with its product | Used only as a factual list of command-to-shortcut mappings. Reviera defines its own shortcut table and must **not** copy or ship these files. |
| github.com/davidvadkerti/customtools `KeyboardShortcuts_davidv_2022.xml` | Public open-source repository; a user's shortcut export | Used only to cross-check the grouping of ribbon tabs, panels and tools. Same handling as above. |
| S0636 lynda.com course page "Configuring connection settings" | Public course landing page (HTTP 200); the course videos are behind a subscription | Only the public search-result summary was read; no course content was accessed. Not cited as the basis of any requirement text (STR-CONN-001 rests on the public help pages). |
| Gap-fill sources S0578 onward marked "search summary" in the agents' notes (two COL help pages) | Facts taken from public search-result summaries of public help pages | The pages themselves are public (HTTP 200 on re-check). |
| `research/03` | Quotes Autodesk Help text heavily | Kept internal. The specification paraphrases it (conventions rule 1.1). |

## A. Sources (710)

| ID | Area | Type | URL | Status | Public-availability test |
|----|------|------|-----|--------|--------------------------|
| S0001 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-453F6BD3-DC8C-4142-9996-F7BCEB6CEB46.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0002 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-2F8F40BC-B0FD-4959-B03B-120494D92665.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0003 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-175F069E-0671-4A97-8361-8BD8EDA7994E.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0004 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-BE73BD2D-2596-43C0-BFD8-FAFFD87253B2.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0005 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-2B906D16-2648-4D2A-999B-FAC175048BF2.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0006 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-1DCA4E6A-F6E9-4DE7-861B-0E66B85A9C25.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0007 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-6B6325AD-EE34-4CE5-844F-F36EBE528A56.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0008 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-48184962-05D5-4F67-88C5-54A872906302.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0009 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-433AEF79-090A-4157-BD6F-273FBA91F826.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0010 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-D705A839-0676-4254-B8CB-8A321C9153B4.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0011 | UI | tutorial / reseller / press | https://www.symetri.co.uk/media/byrpyebk/interface-autodesk-assistant-revit-core.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0012 | UI | tutorial / reseller / press | https://www.symetri.co.uk/media/jgvpl0vh/autodesk-assistant-revit-core.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0013 | ARC, UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-5407BAF8-94D7-488D-83A5-AB3CBE2E044D.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0014 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-865103DF-EFC6-41A5-B0BF-20E55280F3D1.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0015 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-7CDDA0CA-E99F-415F-8F15-017F4406E9FD.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0016 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-ED8D7DAD-268A-48F4-B0CA-21D0C38BBE49.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0017 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-632AB75F-370C-4CD7-94B7-61EB4DDDAFB3.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0018 | UI | tutorial / reseller / press | https://www.cad-notes.com/wp-content/uploads/2023/04/Windows-panel.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0019 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-CCF99547-F6D8-4A0F-908C-49BA85BF3C49.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0020 | UI | tutorial / reseller / press | https://www.symetri.co.uk/media/uvxcoma0/options-bar-revit-core.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0021 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-89105950-A51D-4435-A343-B51C4C1A3E00.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0022 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-2D20DABE-BB53-478E-A0E9-DA371A9A9BE0.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0023 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-EA851C30-BBFA-428B-97B3-041F19889F47.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0024 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-85856A2B-FB2C-4564-8BFE-A9C7F4C016E8.gif | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0025 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-BBEE314E-0C80-4DD5-BE63-1BC832B0BE03.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0026 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-8194A87C-04F4-49D2-9B95-968C700A0027.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0027 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-D1C2D380-CC03-4423-B383-3F1BB515BB23.gif | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0028 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-8C08D236-849B-43D6-9192-A27CF3954ACC.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0029 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-22463280-BE9E-49ED-98D7-FBE594FD5969.gif | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0030 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-962DE732-A549-44FC-9169-158078A5FF05.gif | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0031 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-FF08D2CD-F40F-4D3F-8853-C08F603DCDF0.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0032 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-F8337FFE-5D36-4C26-8B1E-DA2DE5FD48C0.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0033 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-E242DA89-9973-44ED-AF74-BADBA19A361C.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0034 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-FFEF9999-6519-4511-B008-2BE51B343591.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0035 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-563B0990-7903-4E2A-A398-A4D658D99AA9.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0036 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-E39F074A-A1D3-45EF-9853-5A53B1B131E9.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0037 | UI | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-B615F360-DFF4-400E-B1CA-AE9ECC000823.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0038 | UI | tutorial / reseller / press | https://www.cad-notes.com/wp-content/uploads/2023/04/revit-theme-color-setting.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0039 | UI | tutorial / reseller / press | https://www.cad-notes.com/wp-content/uploads/2023/04/working-with-selections.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0040 | UI | tutorial / reseller / press | https://www.cad-notes.com/wp-content/uploads/2023/04/2023-black-background.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0041 | UI | open-source / api-reference | https://github.com/mcneel/rhino.inside-revit/blob/d2c8c9cd6fbcd3ca807bf6aa0e673cc73276dbe0/src/RhinoInside.Revit.External/Resources/RVT2026/KeyboardShortcuts.xml | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0042 | UI | open-source / api-reference | https://github.com/mcneel/rhino.inside-revit/blob/d2c8c9cd6fbcd3ca807bf6aa0e673cc73276dbe0/src/RhinoInside.Revit.External/Resources/RVT2027/KeyboardShortcuts.xml | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0043 | UI | open-source / api-reference | https://github.com/davidvadkerti/customtools/blob/a17abaf582b016ffdbb4df6e014b4b6c708d1f89/CustomTools.tab/Links.panel/Keyboard%20shortcuts.splitpushbutton/Download%20%26%20set%20keyboard%20shortcuts.pushbutton/KeyboardShortcuts_davidv_2022.xml | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0044 | UI | vendor-web | https://www.autodesk.com/revit-keyboard-shortcut-list-pdf-2026-enu | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0045 | UI | vendor-web | https://www.autodesk.com/revit-keyboard-shortcut-list-xls-2026-enu | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0046 | UI | tutorial / reseller / press | https://www.myarchitectai.com/blog/revit-keyboard-shortcuts | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0047 | UI | tutorial / reseller / press | https://www.cad-notes.com/whats-new-in-revit-2024-the-dark-theme/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0048 | DM, STR/MEP, UI | tutorial / reseller / press | https://www.symetri.co.uk/insights/blog/what-s-new-in-revit-2027-a-guide-to-the-latest-features/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0049 | DOC/COL, STR/MEP, UI | vendor-blog | https://www.autodesk.com/blogs/aec/2026/04/07/whats-new-in-revit-2027/ | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0050 | DM, DOC/COL, STR/MEP, UI | vendor-blog | https://www.autodesk.com/blogs/aec/2026/06/16/whats-new-in-revit-2027-1/ | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0051 | UI | forum | https://forums.autodesk.com/t5/revit-ideas/restore-the-contextual-options-bar-for-mep-creation-tools-pipe/idi-p/14187345 | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0052 | UI | vendor-web | https://autodesk.com/support/technical/article/caas/sfdcarticles/sfdcarticles/How-to-set-a-dark-theme-for-Revit.html | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0053 | UI | tutorial / reseller / press | https://ukcommunity.arkance.world/hc/en-us/articles/26023870561810-Revit-2026-New-Features-Part-1 | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0054 | UI | tutorial / reseller / press | https://archilabs.ai/posts/whats-new-in-revit-2026 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0055 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/files/GUID-1CA04013-04CE-4F55-9B0C-68FD7E7FF80B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0056 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/files/GUID-5924644F-C73E-4B76-897D-B181EE9F1B16.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0057 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-10A920FD-1A4C-457B-8827-D36AD7EA2D23.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0058 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-6E5C3207-CA80-452A-9DD8-CA94CD592E51.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0059 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-3E75F88D-D212-43D4-8176-5B7F305B60F2.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0060 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-04D66471-BDEE-444D-BCBA-A0BDAE36032A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0061 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-7657DF90-6AE2-4543-BFDA-7735A8424B1C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0062 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-1CCD1005-CBDA-4338-8D60-489095D4BB25.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0063 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-BC422403-C03D-4292-AD8B-D578EC0659D1.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0064 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-F3B8DD5E-5380-45C1-B0D6-E6B081224E63.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0065 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-BBC16AF1-88C1-4B2E-A23A-917C3F427E97.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0066 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-E5902FF8-506E-4517-9A60-E9C6EA4422FD.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0067 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-98A54E99-6889-4BA6-8483-CE67990A49C7.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0068 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-296F9E46-85F9-4E15-9EF2-39411FA44CA3.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0069 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D4861B15-29FF-49F8-B292-39AD62EBCDD3.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0070 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-0FD4C314-E5AE-4937-ABAB-8EBD95A45695.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0071 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-8EE49924-9905-4E1F-9269-C819B1E1A683.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0072 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-E2E75A78-B698-4518-A5AE-29BFF2BC2688.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0073 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-BA7C38A7-7E8B-45CC-B4A3-950D19B48C66.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0074 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-B8B4C796-200C-4AE6-81B9-8C3889A4AD2C.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0075 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-0CF69C79-1777-48E3-B70F-FF02EE111849.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0076 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-D5AB89BA-5691-451D-BE16-3F3DB5657B76.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0077 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-41D97320-0082-408F-9209-D9B21D94EFD6.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0078 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-4BD87AEC-9389-4C48-B7D6-FBAF446A44A4.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0079 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-674C41FE-039F-4D43-818B-8519D62700A5.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0080 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-858F3507-3F3D-46DF-951C-7B54D2475026.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0081 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-8C6FF952-29DF-4FDC-B207-5CB743DBB63C.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0082 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-A0540715-89B0-4C19-BA45-FA1442B05765.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0083 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-9DF39D7A-3B84-4240-A1B6-91162F7272BE.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0084 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-AD15617A-4114-4953-BDBB-13E0C5A4EA49.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0085 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-ABEA6AB9-D845-408E-B624-7C4986437ED6.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0086 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-4A4BC84F-F17F-4F16-B2B7-8884234806AA.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0087 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-D3F69BE8-0C58-422B-9D78-74130F50A3E6.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0088 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-61292735-2493-4F52-9635-9C9E70829A75.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0089 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-7CE47D8F-2DB0-4F24-AC2C-309F8C3F3383.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0090 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-C7988AC9-A5D9-4801-AF47-3CBD2799132A.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0091 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-10CD000B-3F5C-455E-B79E-8289E2611D66.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0092 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-AAA047CA-90A3-4634-A3E5-EDCCD2CFDD80.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0093 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-917A0534-C8A4-4F62-ADB5-E45C837DF3A8.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0094 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-15A760B9-858F-4C76-90FB-F6C226B86C70.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0095 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-B4AC3088-DBA8-4EB0-BA8F-CAF8EC1EBF9D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0096 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-35C933A7-8ABF-466D-8E01-8571B7BDC03F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0097 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-91CBCCF3-66D1-496B-80B3-D893065D1A50.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0098 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-97612CCF-ADA8-4CF5-9EB3-D6DB37069D06.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0099 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-95485783-6446-408E-A3A2-28B82F497AFF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0100 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D67C16F6-D085-4D45-ADC6-EFD76DE8C672.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0101 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-02B10C4F-A66F-462D-8B7A-6BB3C159F0C4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0102 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-CE5848C0-BCFC-4B05-A76B-B367DEB5FD05.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0103 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-69B6DC75-56A9-4F35-BE24-80048360E2BB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0104 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-F823EB16-8673-4E2E-82DA-4036D05733D4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0105 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-61205D04-381D-4E8D-B971-6ED44126FBDB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0106 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-BF23255E-0FA9-4562-AC5B-2F6AC9FB2EA8.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0107 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-EDD89181-EC92-4191-8E53-BC7881826F73.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0108 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-34DEB2F6-E66E-4801-8138-958DDB4D2A5A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0109 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-71A98877-E4B2-423F-BEEE-94C1305D08DE.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0110 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-6B7015B8-A758-4768-A384-DDBB95814D2C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0111 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-9348929C-18CA-40AC-BB64-71D01CC52F2B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0112 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-241430FC-8084-43A1-AA3A-681B2883B0FC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0113 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-DD74A51D-A0B0-4461-A4BA-0F9CCC191CDB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0114 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-ED1582A8-E1A3-4561-AD8D-50DAC39F43F6.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0115 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-884363F0-3B1E-4894-9CE0-010C4453F448.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0116 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-DA159F50-6AA3-4C7E-8350-7F25541268CC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0117 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-74FFB00B-FB29-44B7-8FEA-613DBE010AD4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0118 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-9D33F884-4BCA-4772-B3E5-1E15A53DEE6E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0119 | ARC, DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-57C738E6-E96B-4275-8F7A-21A268F1576A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0120 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-F26DB6DA-A0EC-424D-B656-3BDF47607F4F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0121 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-565068FC-D890-4977-B420-9481FBAB1E2A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0122 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-718C1341-C4FC-40D6-9646-D2E13A861D33.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0123 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-6B18982C-1FBA-4E5C-AE30-6FAE4022CE35.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0124 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-9607D9C8-537B-4E07-9715-08ECF053AE8F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0125 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-A9AC3FFF-0298-4FCA-896F-86D460A088FB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0126 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D75B2FE4-3FE4-46B0-AC44-49C4A7DBADEC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0127 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-51ED076A-A362-4351-83D9-516E36EE035A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0128 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-A6565ECE-C1CC-48E7-8120-2BC706B196CD.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0129 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-968F4EF1-39FC-4603-A822-D97A215FE77D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0130 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-4B806EBE-2B46-4889-BC12-77FA23BFBA88.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0131 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D54C7E2D-CCCE-4744-9507-0BFCEB6270B5.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0132 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-34EEDC14-4075-4DA8-A7B3-0F8DC4CF14CA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0133 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-156562F5-B4FA-4BB7-A4BC-FA7BB9F2D3C2.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0134 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-3CCCB8E9-2ABB-495D-8B3E-079A4986D65C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0135 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-12CF2B58-39D4-42C5-A3DE-E536ABB4E9E2.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0136 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-94A9F88B-93EE-4213-8991-252E92873FE0.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0137 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-A2309799-1B2F-4AAA-838D-882F8D4D887D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0138 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D843AE7A-457D-4860-9EAE-19FAF171723C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0139 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-02B3706B-3638-4377-AE67-4EEBF1958004.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0140 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-AC61490D-F98E-481F-8779-FC14AF912E95.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0141 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-9BCDC86A-A211-41CA-88C2-99B654472690.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0142 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-54EDD889-9AEA-40B5-BD60-FC2F860DF95D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0143 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-4EA17333-5A68-4E0B-BEDE-7347B4EBDA38.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0144 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-69AACF6E-4130-4499-8160-25DE412D2C97.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0145 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-E8FF6C93-5E46-4459-BE66-A2A606776F79.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0146 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-F384EC11-71AE-4201-9243-6DE820F1E671.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0147 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-9C867721-1970-4ECC-90F1-3C112B9EC2E6.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0148 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-AF2C7508-8B59-4D5C-9E00-3AA98FC67BEF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0149 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-8BAA7C07-D174-4F02-AA81-239FD33BE363.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0150 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-1A7158F1-DC39-4DF9-8D44-DC6933107752.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0151 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-42AA53C5-7BC2-403D-9675-5F41C24212BC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0152 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-836F504C-3AC6-4A4C-BA78-DD65DF734F20.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0153 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-E8F03F7F-8886-4F92-B821-D2C500CEB64F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0154 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-AB491B02-07E9-4ED8-9912-BE9B9BC2440D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0155 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-5ED08D88-8A6B-4609-8E4C-E2C0E7AFB5E0.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0156 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-ED60CE62-B955-45E9-9D7D-00F1552D858C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0157 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-E8B292F4-D056-4228-BD99-2CC99EBA4F58.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0158 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-A964AA13-39B6-4A4C-A230-9F69C8AA5D3A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0159 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-3A26F511-C3E7-4561-820C-62AD520F1358.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0160 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-2C116CC2-F9F9-4238-B7FF-FD6653912D52.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0161 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-805557AB-CB88-4AD6-9753-4CEBD0FB2C78.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0162 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-796061DB-5ED0-49FB-973D-2408CE54BF5D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0163 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-19EAF7A9-331F-4271-94CC-43B76B13011E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0164 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-AAEDDF4E-C84B-4DEF-90CF-B38480AFEA31.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0165 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-ED3765CA-7876-4716-8AB3-7482810BC775.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0166 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-0BB62832-36DD-4E06-A9D4-EE98CE0FCF89.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0167 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-03D76B94-D87E-4B68-8C81-92745075331E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0168 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-F6AD75EF-5D85-4588-8C1D-0E0092319A0C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0169 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-05EEA430-A49A-4F18-BBF7-5042C47469D6.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0170 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-E1479F1A-C311-494C-A90A-1CAD3ED7ACE4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0171 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-65356067-C0C6-4239-8580-08111967713F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0172 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-DE3B04E8-DE3B-424C-977B-2EE9B1A4D171.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0173 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-4460B588-3FF6-4AA5-9E35-2E452E7070BA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0174 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-38BF671F-3F57-4167-B419-92FB2E2F9A9A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0175 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-669094D5-FE7D-4F72-8B09-7BB45479535B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0176 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-62D7C2DF-9E9B-409B-B106-504387438E1C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0177 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-E715D7B4-5C53-4DD9-8F03-FB35981A797E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0178 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-701B3821-2371-4E61-A508-2D76145566A5.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0179 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-A688CED0-E08C-4ED3-A8E8-14CBE78902BB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0180 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-03EABD3A-4736-4762-97F8-F473FEE18162.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0181 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-F521015D-1703-48EA-847D-184E6C277BDB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0182 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-31E2EF0E-875C-4A68-958E-A06E5F2C0838.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0183 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-66BE42AC-C749-4665-9B11-B19185DB1713.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0184 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-2C23930C-1277-4B7A-9EE7-5CCDE59CA6B1.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0185 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-60E69DB2-12D4-42E6-AC9B-5B73F1AB4690.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0186 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-683B5686-405B-4B0F-BEBB-465EA61C61D3.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0187 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-1418B04D-FCF6-4918-A385-3CDEE75CBA38.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0188 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-BBEA4578-C1F5-4D56-B7AE-2158A0B9045A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0189 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-1480F70D-E715-4B62-8448-E3A8A3823A8A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0190 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-5C1BAC7E-E29A-4134-8EB6-EDE4437B8C36.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0191 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-1674C690-A934-4AD9-8C3B-A2624B1EC46B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0192 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-BD09E46E-2A8C-4C40-A809-64BFF366B6BA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0193 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-E0DB4409-4E19-421F-ABCF-4DDA3A0F9953.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0194 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-4EE804D4-CF0B-46C9-AE61-339B78E4EA8D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0195 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-FF744EF9-336A-4618-A80E-F91866359C68.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0196 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-B3420A54-A9BC-4AEE-A07C-CD7A9DC782FB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0197 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-EA5A34CB-21A3-45CB-B5A5-5558D88BBE94.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0198 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-B9C2DF17-26D7-48D0-ADD9-422C9AA39A8D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0199 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-0EF16913-06FF-4AA8-B163-52F40849520F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0200 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D35D5189-DFC8-4652-8A96-5AF3D5AF9F5F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0201 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-DE78D4F9-F26E-4653-B4DE-90AD55F37921.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0202 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-E69DE397-E2DF-491C-8B52-ACE952286DAD.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0203 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D41E5FFB-CF2B-4621-A873-166209C1ADA8.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0204 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-21AB5490-9DCA-4716-9823-DAF47A530575.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0205 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-D237E6DA-CC5B-4BBE-86E1-3C8AC28CF8AF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0206 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-693AE6F9-C37C-458F-9259-06F183EC068F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0207 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-68DBFA52-DF77-4B3F-84AC-F20194FADC64.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0208 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-A180B32D-E7B8-474F-AB31-B15EDB874F6E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0209 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-560F38C4-ECA4-4220-A935-1E29BEDEE923.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0210 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-C3F4D1B7-83E0-4354-919A-101D6B86A220.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0211 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-7D7C1D92-5318-4BF9-99C2-10A45113E63F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0212 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-905E35DB-1D6B-48B4-AB9B-866F3AECD2E6.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0213 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-B1E3A57E-5843-4E6C-8A05-9639A4D84864.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0214 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-A9DF72D1-6AC9-429F-A67B-A7F998645FD1.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0215 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-B9263125-74EA-4C78-AAAA-F40916EFE2DB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0216 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-5257474E-2718-4789-A46D-31376ABFFBE7.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0217 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-DE386617-D2BB-4F71-96B1-51D157DAEAA7.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0218 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-FA0CF7B6-D3E1-4F58-AE02-A19457BBCD02.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0219 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-0FBD7B6C-7D98-4E55-9889-D800675FDC35.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0220 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-FE932AF0-4C84-4D59-B0AA-08F3E6D7A9E8.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0221 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-4D32147D-EC15-4203-A6F4-C69B243CCEED.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0222 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-9E6DF30D-FDF4-4B9A-849A-C3DE46A31E7F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0223 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-F9DFEDF3-FA2C-4A9D-80A4-DDDA1898A0E4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0224 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-10DB1268-CA74-4620-8A1B-AF412BCD36D6.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0225 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-C13AC7DE-2F62-45F7-92FA-0C52E49E2668.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0226 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-08C6541F-4974-489E-B941-F365628B2884.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0227 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-08C23A81-0739-4226-BF63-AC891135748F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0228 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-426BE12E-A906-457B-A205-A5DFF08B07D7.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0229 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-997A52B4-7DB2-4125-9DC8-478F6A751E36.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0230 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-849104E7-41EB-4644-9A0E-EE24A6E49B10.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0231 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-B6EEDFEF-641E-409A-8C42-AB5A66BDC554.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0232 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-84D8BC77-AAF3-4F45-B77E-58E35858D3AA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0233 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-1121752C-EAB2-411F-A549-66E599A820B8.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0234 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-C971CE95-3F98-4E77-8F26-4AF2D26B36B4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0235 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-8D52AEF2-A36B-4952-BFEC-91E4F2B51793.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0236 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-5220FA75-4761-4277-9FE0-6E9370D5AC4B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0237 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-ABFC9E84-25C0-42DC-9F2A-46D7B8866497.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0238 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-48E8B208-9E82-454D-B4D4-395C3B7C0894.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0239 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-E4F66BE4-88B7-46D3-8BDB-F2BFBBA3A912.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0240 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-B439B9FB-E672-4A26-B239-F254610A690B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0241 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D69E6B7D-8D8D-40E4-A6D4-7CAF1A927894.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0242 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-167B3F56-3CAA-48BB-96D6-966296B36EA8.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0243 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-134DD38D-5D73-4044-88F0-E215D04F945F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0244 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-F3D6884F-F1E3-471B-81FB-ECE93D9C2671.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0245 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-C1B7C157-5E55-4AF3-BBA7-B6FF8EBA7AF0.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0246 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-5F22D076-345B-4539-9362-483CF898A77E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0247 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-ED2F9C35-5051-4744-ADCF-D3EC1597A1AD.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0248 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-11B0CBC7-BFFE-4E9B-9B23-8F7D76473BD9.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0249 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-DF735D0A-A051-43C0-B21B-E8F6CBAFD5C3.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0250 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/files/GUID-E66946AE-64B0-448D-8357-65F30F643F39.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0251 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-93F44C38-8158-4F7D-94E3-C076E1367786.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0252 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-0F4062E5-3826-47B8-BD6C-0D3807AA76A5.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0253 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-48EB9CC8-BF75-435E-AC99-D34726490AFF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0254 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-135437DA-A8DF-4819-BF0D-C5884431656F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0255 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-D7240EA9-28EB-4F51-A474-371FA0DFBFAF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0256 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-3720B726-E2A0-4E52-90BD-439829564E38.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0257 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-399D3557-93A0-42B7-AF77-E28931823335.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0258 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-49C8AE85-3E9D-4695-A4C1-0E7950FB57FF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0259 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-7D416FFB-36A2-4B98-95D1-0A007DCD51CE.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0260 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-91F4AC32-22AE-45C2-BD6E-01164C4E6EC1.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0261 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/files/GUID-30D3A26F-42FF-40FA-83D2-B43EA67F3ECD.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0262 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/files/GUID-E325CDE7-5468-43C8-BEAD-EC5AF13231F0.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0263 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/files/GUID-02936CA2-FE9E-4625-A2A6-A45324937847.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0264 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/files/GUID-8942AAB6-60C0-469A-8C39-5950E3B96716.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0265 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/files/GUID-82E544DF-E250-4F7C-BE5A-4E83989F85E7.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0266 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-4E8562EC-38C4-485E-A86D-D459096187AF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0267 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-09AD3010-7186-45FA-BC26-C2AD843B6ABE.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0268 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-7E952007-8A5A-4693-8E0F-A847F72F7617.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0269 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-7E952007-8A5A-4693-8E0F-A847F72F7617.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0270 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D8DADD9D-11C2-4824-817E-EE3D9F5C7C8C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0271 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-234F69C3-516A-4866-91D7-81B762170D4E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0272 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-BBD3A057-E79F-43CB-A388-95625C0E1096.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0273 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-05BAFEAA-5186-484E-80F4-8D900C454748.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0274 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-05BAFEAA-5186-484E-80F4-8D900C454748.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0275 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-3B7BAC64-6120-4372-A29E-95C4A1D853F5.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0276 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-374DEF0C-9EE0-4499-9FA5-BB7D5555A384.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0277 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D94E11E7-0956-4B5B-B50F-6BB28AD09F15.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0278 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-74EB2807-8377-4A24-A2B0-7C01163078CC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0279 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-445AD892-BD7F-4D5B-A6C4-C5F5F87228AF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0280 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-6385104B-4295-4659-855F-BA7DD2793CD5.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0281 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D0A28BE6-BDD0-43D3-A2A5-3F0C867C55E5.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0282 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-6DF71CA9-DBDA-4A5A-8252-48AD331403DA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0283 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-682B3B19-A4F7-4EE7-B994-F3EA3154BB08.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0284 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-A5745932-897D-4F28-85B8-F5B70BB661DD.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0285 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-21326970-0037-41C6-A996-980C24EE019F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0286 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-9C25D11A-C4D3-4B0A-9793-45E82464840A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0287 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-B0BC7A1A-1EE8-4034-9793-98D627119B90.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0288 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-CCDEE011-2A5E-43AC-BD60-8F81CF432A6B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0289 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-22EC1748-A525-43BB-A45E-97D104CF303B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0290 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-7189B9BD-8028-421B-967A-8DEA8B25BDC9.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0291 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-E64739B6-E5DA-46FE-B3C1-CA4D295557CD.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0292 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-26053199-6B08-44B9-907C-F5D9AA7CEC81.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0293 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-F94A83E9-F911-4A64-BF05-089CBDB92257.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0294 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-3426ADE0-6B76-4453-87EF-653DAE6BCBEC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0295 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-1AEAFD34-194F-4913-8020-971FC9A0A218.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0296 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-F84EC15A-3414-4CC2-ACD0-027033DBCE5A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0297 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-09956850-F036-49C7-81DC-B9296ACF53BA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0298 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-1EA6095B-5B2C-47EC-96F1-6FCA9D09097C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0299 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-1440031D-3747-4525-9FA8-FC74A45BEBD7.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0300 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-E7F7A8AD-69DC-4C55-A3E2-A9E844BC9571.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0301 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-1BE1E53A-4123-4068-B6EF-FFC64DE6A813.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0302 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-130A65BC-8C57-4C31-8C07-F96041771539.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0303 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-93DD8AD2-B056-427E-A04F-948D451B7CFF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0304 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-40DAA2D0-9797-4937-AE6B-51EC92D599BC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0305 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-17697733-43F6-4A80-8D0D-0526823ABEA0.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0306 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-567D0E2A-E406-4B47-86FD-3C7170F0B83B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0307 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-50482AF3-42C3-4993-A7B5-EB3E1C17FAE1.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0308 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-719EDC0D-BB98-4B25-B7A6-35AF208970D7.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0309 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-541EB756-8376-4522-BFA1-141C22346ED8.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0310 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-E36AE889-D507-4288-90BE-FCE2F120C2B6.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0311 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-049762B4-F052-4F87-ACBD-BF757A26178E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0312 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-5FBF2AA2-B811-4443-9DBB-9DE62191F332.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0313 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-3BE4F02E-3861-4B6D-8929-97F9ABFBDB91.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0314 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-B5AAA3F6-1AF1-4D02-96C8-91E61E656CEB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0315 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-03955E27-EFD7-4ED2-8DC7-FC321FE43F12.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0316 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-40313D24-AB5E-4053-98B7-7B82B3C60B34.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0317 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-21047A44-6030-49E8-B1B9-03366EA6A0C2.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0318 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-D9AB5E1D-2D76-4D92-8000-24B78AAF09CC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0319 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D6880A68-2A18-4515-863A-3BF42A3B5A12.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0320 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-5570AABE-34F0-45B7-B42A-EF3E99309EA5.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0321 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-84FFE118-6EF7-4747-8DF7-AE675818645E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0322 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-D9EDB8DB-2D08-40B2-9979-2F1A5B5962F6.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0323 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-4E763B5A-1FFF-4084-BFAC-B587FEB01B3D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0324 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-CA3A831E-B9B5-40DE-BBEA-9D11AF9840EB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0325 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-96E50D3B-2CF9-4F47-9C40-B689D114C521.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0326 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-ED2045E5-90BF-418F-AAD6-3BB1544F34F5.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0327 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-61B98A58-B03B-48D7-B2D9-99A564FC55D0.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0328 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-C6A3DD86-FFAF-4781-8E50-5714965C24BE.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0329 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-2AF7D31D-A49C-4E10-897E-635D5DE60403.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0330 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-1D0063CA-41AE-47CF-A6A7-0025DC48B7FD.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0331 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-50F25D06-C5CD-42B9-A766-CAABD3F2FA91.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0332 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-736C518A-28E7-4271-83B8-728CBBB6DC17.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0333 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-5BFDFD63-0E4D-4684-BC3D-DBB165CF41CA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0334 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-62AF960B-1411-4C3B-8666-8A22F09DF28A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0335 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-09427894-9D5A-48B0-8CEF-4E9EC4D608E1.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0336 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-0489005A-5724-4933-8894-5FCABA00A0EB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0337 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-83FC7BC8-61F1-4636-AA04-D54350682C70.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0338 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-3D7247D9-BC7E-4E2A-A604-3068CDFC4861.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0339 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-F54D8DE1-8700-49F7-BED6-54B3F56D85BC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0340 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-73A2A764-FB3B-4905-ADB5-76056C3DB089.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0341 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-663D1B7B-092D-440D-B1B5-5AD6DD43158F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0342 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-362E71F1-45FE-41A1-91AA-DC84FE42F6BD.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0343 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-332C00ED-8157-495E-9EF9-250B32E4A380.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0344 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-B15F26EC-57C1-4570-8EAD-1A0C8089A23A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0345 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-66F91C59-4159-4A40-B894-178732DB596C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0346 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-2FE92E3F-5F4A-4C4D-8272-EF24115690AC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0347 | ARC | tutorial / reseller / press | https://www.scan2cad.com/blog/wp-content/uploads/2020/10/Revit-Keyboard-Shortcuts.pdf | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0348 | ARC | vendor-web | https://www.autodesk.com/shortcuts/revit | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0349 | ARC | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/data/toctree.json | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0350 | ARC | vendor-help | https://help.autodesk.com/view/RVT/2027/ENU/data/toctree.json | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0351 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-14D5F295-680A-440B-848C-847363BA0D06 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0352 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-9D989FA4-FE1F-4E85-B99D-BD33E005B420 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0353 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-CACA6719-A8E6-4526-9171-1859315BDAA2 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0354 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2027/ENU/?guid=GUID-C79E0E2E-6A74-4544-A134-A9167AA62310 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0355 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2027/ENU/?guid=GUID-E0821FE8-B1F1-4C4C-9350-C56D0DA6DD75 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0356 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-48C77E15-725F-413F-8001-6A1EBD03A1D7 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0357 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-BE343C51-5F71-488E-BFCA-5DB4D5CF1106 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0358 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-83CC4548-F220-4C5B-A50B-82D42CE03460 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0359 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-537703E1-9AA8-49E6-9835-7D779C3FD547 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0360 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2027/ENU/?guid=GUID-FC084260-A0E9-4F62-A136-6CDB3E514BE7 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0361 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2027/ENU/?guid=GUID-515DEE6B-45D1-4A7E-867E-7330AE877B51 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0362 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-885D38DF-0FAE-4D35-B5DA-2FE75F3B420B | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0363 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-0CB9D92E-F41A-46AB-A7D6-955F4B831425 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0364 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-C38F239F-F919-499F-9F13-269C95E460BE | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0365 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-90168BF1-32A4-40DE-819F-8D91999BBE14 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0366 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-416C1FF7-1B42-4C17-ADDE-DC9D9954E49A | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0367 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-2D2B6562-4997-4A80-8FFF-E0E35127A6E7 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0368 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-7367C537-C738-478C-B4EC-F9E5BBA0B8B8 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0369 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-804F284A-6CE9-4311-BD2E-90748EADD5B9 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0370 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-D2362C11-C604-4B32-AB76-19E6EEF75439 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0371 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-3F5CF335-1F1F-46D3-A781-3FE0B6209E57 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0372 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-6A8EC243-9414-4530-96B9-A413594A8B54 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0373 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-A1B4B240-6EE4-4CF8-A9EE-AC989CC1FCD3 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0374 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-692C4E68-6A9D-4628-8E73-0C327E5677D4 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0375 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-C4115E01-4FD5-406E-B738-5AB927235240 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0376 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2027/ENU/?guid=GUID-63B2E042-CA8B-46FB-88CB-FD7ADA002973 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0377 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-EFFCBAEA-88B8-412C-B1DB-D0DFCBE15810 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0378 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-6E877A24-B52E-4039-8F44-1FBABB156189 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0379 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-StructEng/images/GUID-0E2867A8-9AA0-471E-82E5-FE7ED8EDFDF9.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0380 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-StructEng/images/GUID-3A081CC9-BDD5-486C-9461-F746F727BD8B.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0381 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-StructEng/images/GUID-60A74D82-6E3D-4ACF-AC5F-FA8C030B9848.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0382 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-StructEng/images/GUID-4CA208A0-477E-4242-95F1-DF9B9B0BB4FE.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0383 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-StructEng/images/GUID-24C82F12-44ED-46C7-B269-9153038C13DA.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0384 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-StructEng/images/GUID-766EEB17-2F8C-4F10-B036-1B96585204AC.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0385 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-StructEng/images/GUID-6DA18F47-FE13-4808-9C2B-880EE5353344.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0386 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-StructEng/images/GUID-274226B7-80F8-420A-A4C9-CDA7B740784A.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0387 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-StructEng/images/GUID-365E0138-294A-4F6C-B8A4-83332CC9DDDB.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0388 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-StructEng/images/GUID-A54AFAA6-A537-4692-810E-64928E16B40E.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0389 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-StructEng/images/GUID-8C011FA0-01E1-472F-A7CF-7201DBE13DF2.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0390 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-StructEng/images/GUID-915A8BE5-A2FC-49B8-9FC3-EAD1C3DE531B.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0391 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-StructEng/images/GUID-C3E6449E-716B-41F8-8CB5-78861BBF6285.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0392 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-StructEng/images/GUID-F8C7A333-7FF8-4AF5-BBB8-3F417EE9277E.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0393 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-StructEng/images/GUID-FA140FE7-0649-48CE-96C2-C7777B6122F9.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0394 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-MEPEng/images/GUID-2D3B241E-C353-4ECF-BFC6-0BCBAF8573F7.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0395 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-MEPEng/images/GUID-612911B6-3C23-4E70-B42B-D2FA32780384.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0396 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-MEPEng/images/GUID-2C64CD7C-3F49-4B0A-BB28-85FE2BFE2336.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0397 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-MEPEng/images/GUID-3BFA264C-D69A-4EAF-9E3E-C1ABE988CC53.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0398 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-MEPEng/images/GUID-00A13C1F-F194-4384-9649-BEEF2062F1BF.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0399 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-MEPEng/images/GUID-CD89CE99-6263-45D8-87B9-3013614C3D63.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0400 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-MEPEng/images/GUID-2A963BD6-80B6-418E-A936-91128BB04ACC.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0401 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-MEPEng/images/GUID-3343D830-FEDD-41A7-9277-A5DBBF0F70B6.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0402 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-551AE7F4-B53F-4BA4-92D8-37D07FB487F1.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0403 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-BE843CEB-B705-4359-B2B3-DD4338A6010F.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0404 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Analyze/images/GUID-BCC86D32-5742-42BD-917F-DABA890EDF3A.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0405 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Analyze/images/GUID-E168531C-0A1B-4E21-A711-D054C2907D0D.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0406 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Analyze/images/GUID-62540F3B-A4EA-48E3-968E-D394F3E655CB.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0407 | STR/MEP | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Analyze/images/GUID-EEC0CE10-1E2C-4C1B-BA12-9E4620557CB1.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0408 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2026/ENU/?guid=… | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0409 | STR/MEP | vendor-help | https://help.autodesk.com/view/RVT/2027/ENU/?guid=… | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0410 | DM, STR/MEP | vendor-blog | https://www.autodesk.com/blogs/aec/2025/04/02/whats-new-in-revit-2026/ | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0411 | DM, DOC/COL, STR/MEP | vendor-blog | https://www.autodesk.com/blogs/aec/2026/07/22/whats-new-in-revit-2027-2/ | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0412 | STR/MEP | tutorial / reseller / press | https://thecadmasters.com/whats-new-in-revit-2027/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0413 | STR/MEP | tutorial / reseller / press | https://thecadmasters.com/whats-new-in-revit-2026/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0414 | STR/MEP | tutorial / reseller / press | https://symetri.co.uk/insights/blog/how-revit-2026-can-support-your-workflows | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0415 | STR/MEP | tutorial / reseller / press | https://ukcommunity.arkance.world/hc/en-us/articles/34851925379218 | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0416 | STR/MEP | tutorial / reseller / press | https://ukcommunity.arkance.world/hc/en-us/articles/34884025793298 | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0417 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-58711292-AB78-4C8F-BAA1-0855DDB518BF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0418 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-1245116F-DF47-4787-8C86-0607822CC702.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0419 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-C8EDE015-4AE9-4A49-AB1B-AAEE04845B17.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0420 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-B05CE566-A607-42DC-99E4-7FAF442B31BA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0421 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-BC219BE7-2917-47A8-9596-71C6C01659CF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0422 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-4ED30197-CE07-477A-A093-6C397DE446CC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0423 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/files/GUID-D42C5877-6384-46D4-BEE3-195F3DCDECF1.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0424 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-997DA39A-EC39-41BF-ACAC-975087E986E7.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0425 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-745313E6-93F6-4FA2-A7D0-7D45242F5520.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0426 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-6D569C3B-B473-4AA9-BB2F-B93088D7D9A9.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0427 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-C3BE8E62-7D0B-4B5C-B8DD-7D4CDDA960CE.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0428 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-D8D3BAF0-587E-4D08-885C-FFC3504F056D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0429 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-685D80C6-279C-4C47-9826-1483DDD0312E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0430 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-3D4D534D-DDC8-4BBC-BC36-300566FA0766.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0431 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-BECFA1C6-5A10-4A40-92CB-0DCBE07D434C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0432 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-AC50D6C6-56E2-40BF-96BF-EC1E31E0020B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0433 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-0FC44807-DF06-4516-905A-4100281AC486.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0434 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-61C821EE-970C-46CC-B3BF-D03BE88E4288.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0435 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-7AF97381-C703-4ACE-8153-E328605CB018.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0436 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-67BDEB80-2EAA-4F66-8562-D7C42A0F3A09.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0437 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-014966A2-2524-40F5-B033-B3E891BD2A59.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0438 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-6A29E0EC-4720-498F-9C7F-F1BD97DDBABB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0439 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-5E8B0871-E4F0-49A0-9B8F-E36C495E04C3.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0440 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-C0154226-DBE8-4724-8486-2BE7AB4BBB61.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0441 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-2AA07A43-A4BE-4AD4-BFC4-7F028126E4A1.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0442 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-6DECE749-0303-4FEA-8CE2-A916C7A08863.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0443 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-20F5F6B8-F713-4BCB-B8AB-B484640A2D7B.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0444 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-CD2D084C-0A4B-494E-8FD5-CF5344CE68AF.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0445 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-31BF4AB2-BCF6-4856-B255-AB7218BAD1C5.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0446 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-1D983C33-1675-428C-9CDE-76AB2B49AFDD.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0447 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-825E58B6-CB9D-401B-A597-7259E57A8EBD.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0448 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-7F34EF17-988D-45A0-8FA6-867D0DCD472D.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0449 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/images/GUID-720FC0D0-C914-429B-8659-1ED58D0C7747.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0450 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/images/GUID-3687A9EB-69D7-41B3-BB97-CDF94AE59226.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0451 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/images/GUID-ED76C489-B63F-409B-900B-269EB4E63C9F.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0452 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-A7F33267-8DED-4E56-9635-80EB8B05AC9F.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0453 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-3E779A13-18AC-496F-B68B-6088B4A9A19E.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0454 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-C58377E3-B6D7-4733-A827-3E637010C0C4.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0455 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-3E55A869-78D4-44B6-93BE-6D6F28DD86C8.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0456 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-EF354517-3BFC-498D-9DB5-EDB433E6E8F2.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0457 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-464D9BA1-CADE-4BC9-8630-7B7F4931FFFA.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0458 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-CED81A83-42BB-4702-9283-02D6CC07B935.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0459 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-3013746E-B14F-4353-B4E3-69A8BA492093.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0460 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/images/GUID-A7FB7D75-7F78-4B62-B134-12BAD26E7528.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0461 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/images/GUID-C200280A-5C9C-47E4-938C-AC3E70C584E9.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0462 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/images/GUID-808B54D6-90E4-4167-9527-6D779416891E.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0463 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/images/GUID-4F82CDC3-0452-4CB7-93A3-314758639F58.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0464 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/images/GUID-6964D002-2562-40D9-8D4A-C5C4BB3115CC.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0465 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/images/GUID-7B5F92D1-ECEF-47B1-88D6-8C3C726EC4C6.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0466 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/images/GUID-C7767FD0-A670-49EB-938A-466955745242.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0467 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-C8976D73-6590-4D65-9B41-626F48F4D9BB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0468 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-CFE335EC-D17A-447C-999D-4D2E91C62C77.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0469 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-6AFBA7D1-D5BC-4B80-B889-B6D2409CA875.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0470 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-F7D48874-EDB0-454B-8E5B-19C551A78978.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0471 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-D3E7FDC3-57AB-4497-AB87-D195476254C2.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0472 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-E2E171B1-3E6C-4E16-93E6-958175806BC9.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0473 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-67176ECE-48EE-4229-BCB5-C4871B99D36B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0474 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-773AD069-024B-425E-8B9A-05D5246BDD16.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0475 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-42C75024-4D71-4831-8910-2747168624A3.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0476 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-E8443B4B-D55B-4630-BEE3-D2D8628CA17B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0477 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-6EB68CEC-6C17-4B16-A509-30537F666C1F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0478 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-E029E3AD-1639-4446-A935-C9796BC34C95.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0479 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-98192ABD-4E6F-4AF5-B909-FF0795B0AA39.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0480 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-74175200-67BE-48AE-901F-228FD96C3DE4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0481 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-C4E941F0-C469-4D7F-B441-849606D9A52D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0482 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-A16386E9-4514-4375-B76A-1A6E5FCF4B33.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0483 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-06B93886-3D55-4967-ABE2-4B6B262663CD.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0484 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2023/ENU/Revit-Collaborate/files/GUID-890A9FE0-EFF4-4CFB-9E81-B0DE1A132BEC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0485 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2023/ENU/Revit-Model/files/GUID-9DD9DCDB-F80F-4FCE-BA87-FE49B66936CF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0486 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-0C6D8890-73D9-4DAC-A0A6-84C45E54C6A8.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0487 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-WhatsNew/files/GUID-C81929D7-02CB-4BF7-A637-9B98EC9EB38B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0488 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-C81929D7-02CB-4BF7-A637-9B98EC9EB38B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0489 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Cloud/files/GUID-ABFFFEE6-2C6D-4779-B508-A1C4BC0C4885.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0490 | DOC/COL | vendor-blog | https://www.autodesk.com/blogs/aec/2025/11/11/whats-new-in-revit-2026-4/ | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0491 | DOC/COL | vendor-blog | https://www.autodesk.com/blogs/aec/2024/04/02/whats-new-in-revit-2025/ | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0492 | DOC/COL | tutorial / reseller / press | https://aecbytes.com/tipsandtricks/2025/issue111-Revit2026.html | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0493 | DOC/COL | tutorial / reseller / press | https://www.aecbytes.com/tipsandtricks/2024/issue107-Revit2025.html | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0494 | DOC/COL | tutorial / reseller / press | https://graitec.com/sk/blog/novinky-revit-2027/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0495 | DOC/COL | tutorial / reseller / press | https://www.bimpure.com/blog/new-revit-2026-feature-accelerated-graphics | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0496 | DOC/COL | tutorial / reseller / press | https://resources.imaginit.com/revit/revit-cloud-model-save-vs-sync-vs-publish-simplified | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0497 | DOC/COL | vendor-help | https://help.autodesk.com/cloudhelp/ENU/ARENDERING/files/GUID-9AB24E42-4D4D-4AA0-B166-296D10531A48.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0498 | DOC/COL | tutorial / reseller / press | https://novedge.com/blogs/design-news/revit-tip-mastering-revit-view-range-for-clearer-plan-documentation | 429 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0499 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-6CC4C825-7833-4169-9B8E-D244DDD3202B.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0500 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-547344A9-43D7-4B4B-AE05-4B95D85292C5.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0501 | DM | community-wiki | https://static.wikitide.net/revitwiki/7/7c/Family_Parameters_-_Common.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0502 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-AA6627CC-24AA-43AD-8B10-3C1D7C846237.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0503 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-BFE9DB84-B549-496A-B2A5-DFAE8F28E740.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0504 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-DF875F40-BB7D-4E02-BF8D-2A552EEAFE21.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0505 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-BDFC8FB1-7042-404A-A284-282540137044.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0506 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-9C2CD26E-A166-411E-B8C7-90A0D2203AB1.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0507 | DM | tutorial / reseller / press | https://eadn-wc01-10977100.nxedge.io/wp-content/uploads/2021/12/edit-shared-parameters-1.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0508 | DM | tutorial / reseller / press | https://eadn-wc01-10977100.nxedge.io/wp-content/uploads/2021/12/parameter-properties-dialog.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0509 | DM | tutorial / reseller / press | https://www.cad-notes.com/wp-content/uploads/2015/06/Revit_project_units.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0510 | DM | tutorial / reseller / press | https://eadn-wc01-10977100.nxedge.io/wp-content/uploads/2021/12/project-parameters.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0511 | DM | open-source / api-reference | https://primer2.dynamobim.org/7_dynamo_for_revit/1-the-revit-connection | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0512 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-9689D74C-4C7F-4801-A14A-87DDF6C2B0A4.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0513 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-7B935C59-BF24-4618-B441-B9B1B7C19888.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0514 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-041F5DC9-1961-4BC0-813F-39C2979DBE4C.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0515 | DM | tutorial / reseller / press | https://kajabi-storefronts-production.kajabi-cdn.com/kajabi-storefronts-production/file-uploads/blogs/2147511612/images/3675aa-7d7c-6c5b-e4d-cf084cb2414c_carbon-information.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0516 | DM | tutorial / reseller / press | https://kajabi-storefronts-production.kajabi-cdn.com/kajabi-storefronts-production/file-uploads/blogs/2147511612/images/446dcd7-6f8c-7ff1-2815-0bd85ac62b_AI-Assistant-Issue.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0517 | DM | tutorial / reseller / press | https://kajabi-storefronts-production.kajabi-cdn.com/kajabi-storefronts-production/file-uploads/blogs/2147511612/images/a3ae7d8-5530-1c55-b81d-8d6cdb5261b2_ifc-mapping.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0518 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-F6D4C173-6DBD-4F7E-80DD-69F8AB5320E4.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0519 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-E8CD1B40-782B-486E-B691-80CFD8D53DF1.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0520 | DM | tutorial / reseller / press | https://www.cad-notes.com/wp-content/uploads/2019/02/dynamo-working-interface.jpg | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0521 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/images/GUID-C36E20B6-0E91-493D-91A1-BAA53703185F.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0522 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/images/GUID-4C86274A-864A-46B6-BE73-861614CB75C0.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0523 | DM | tutorial / reseller / press | https://kajabi-storefronts-production.kajabi-cdn.com/kajabi-storefronts-production/file-uploads/blogs/2147511612/images/4f3d031-0cd6-84e0-65fe-35b028d34a7c_wall-tool-r-2027.png | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0524 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-0AA0E65D-55A4-4391-AA29-C53C06C048F4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0525 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-26E4614F-6BAF-4F63-A285-99F1B3BE02F5.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0526 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-8D1A49AB-849C-49DF-A7B9-34C596E0C6F2.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0527 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-3F38AE40-023F-415A-AD35-645635D963F8.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0528 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-921F7A15-D191-4F75-8243-4989C482E253.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0529 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-68EFCA67-4913-4E00-AB9E-F2E6A7BEF8C6.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0530 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Customize/files/GUID-253B2300-35C2-4024-AB70-43E576CEA49C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0531 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-377AA649-AE44-4F1E-A588-1C9590D005F4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0532 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-1CFA64E0-0CD7-4529-B50C-26FA129419AB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0533 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-26CE1E00-72C1-49D6-9295-A59F50BAA0EF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0534 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-E4B46475-9041-48C7-BB31-859A06A3B1BF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0535 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-DBC31A77-813A-47E0-8EFA-B6D821F75EDB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0536 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-7610D11E-D1C9-44AF-9B0B-4A75EAC709B9.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0537 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-94EA2B8E-2C00-4D29-8D5A-C7C6664DE9CE.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0538 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-114C68A9-FF2B-4D4C-A2DF-3AE7E1F174C8.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0539 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-05406C56-B144-46FD-AD28-EE24C02BE1A9.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0540 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-30CE876E-36A1-4753-B482-424FC891C470.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0541 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-30290298-D23B-40FC-8642-050C3BD1830D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0542 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-3128F39D-34A3-4D09-A5BC-86C8D7300895.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0543 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-2A49FE80-1B07-4C95-A0FE-42CE2001F3C2.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0544 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-85FA464E-D604-4242-9711-6183A5EF7F0A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0545 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-CC7908A4-5952-4D33-BC05-1947C829D5B7.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0546 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-3F81E8AC-757A-4F93-B7E5-FD06DBB36F3A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0547 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-BC84C446-33DB-45C4-AD8D-9F5DD9BEF78A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0548 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-EFF4E5A3-16EB-4CB4-AD6A-5EEE823CCE76.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0549 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-071913D8-214A-45AB-A798-A81653E77F88.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0550 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/files/GUID-D3A09447-BA7F-4045-88BF-EA8B37F0D46D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0551 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/files/GUID-E8205058-9CA4-4CFF-95F5-A6AA1CDA7346.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0552 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/files/GUID-D6E187C0-874C-4C9A-B6B7-EC8404F75B9B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0553 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/RevitDynamo/files/RevitDynamo_Dynamo_Player_html.html | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0554 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2017/ENU/Revit-API/files/GUID-8EB25D2A-3CAF-486A-BA8E-C2BEF3DB68F6.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0555 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2016/ENU/Revit-API/files/GUID-7577712B-B09F-4585-BE0C-FF16A5078D29.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0556 | DM | vendor-web | https://blog.autodesk.io/revit-2027-sdk-net-10-api-changes-and-additions/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0557 | DM | open-source / api-reference | https://rvtdocs.com/2027/whats-new/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0558 | DM | open-source / api-reference | https://rvtdocs.com/2026/whatsnew | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0559 | DM | forum | https://forums.autodesk.com/t5/revit-api-forum/revit-2025-macros/m-p/13173113 | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0560 | DM | open-source / api-reference | https://packages.nuget.org/packages/Revit_All_Main_Versions_API_x64 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0561 | DM | vendor-blog | https://www.autodesk.com/blogs/aec/2022/12/04/whats-new-in-parameters-service/ | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0562 | DM | tutorial / reseller / press | https://ukcommunity.arkance.world/hc/en-us/articles/21566328135314-New-things-to-see-in-Revit-2023-Cloud-Parameters-Service | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0563 | DM | tutorial / reseller / press | https://www.bimpure.com/blog/revit-2027 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0564 | DM | tutorial / reseller / press | https://www.bimpure.com/blog/top-5-new-features-in-revit-2027-1-and-revit-2027-2 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0565 | DM | tutorial / reseller / press | https://www.bimpure.com/blog/revit-2026 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0566 | DM | tutorial / reseller / press | https://www.bimpure.com/blog/revit-2025 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0567 | DM | tutorial / reseller / press | https://www.bimpure.com/blog/revit-2024 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0568 | DM | tutorial / reseller / press | https://www.bimpure.com/blog/revit-2023 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0569 | DM | tutorial / reseller / press | https://microsolresources.com/?p=16900 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0570 | DM | tutorial / reseller / press | https://www.renderahouse.com/blog/revit-system-requirements | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0571 | DM | vendor-web | https://www.autodesk.com/products/revit-lt/product-details | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0572 | DM | tutorial / reseller / press | https://thecadmasters.com/shared-parameter-in-revit/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0573 | DM | tutorial / reseller / press | https://resources.imaginit.com/building-solutions-blog/investigating-revits-shared-parameter-file | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0574 | DM | community-wiki | https://revit.miraheze.org/wiki/Family_Creation | 403 | PASS: public page; automated access rate-limited or blocked (bot protection), no login required |
| S0575 | DM | tutorial / reseller / press | https://www.cad-notes.com/revit-units/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0576 | DM | tutorial / reseller / press | https://www.cad-notes.com/introduction-to-dynamo/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0577 | DM | open-source / api-reference | https://github.com/DynamoDS/DynamoPrimerNew | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0578 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-1A244A01-DED4-46AA-909D-14A78A48A98E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0579 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-ADFB8EA4-191A-48B7-ACE3-436F89FB68EF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0580 | DM | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-1F177E3C-6C6B-4D0D-BD5B-D1904EB3120D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0581 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-84FFE118-6EF7-4747-8DF7-AE675818645E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0582 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-GetStarted/files/GUID-0B88E1B2-37D2-4008-A2B4-50C35F1DB147.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0583 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-F0C15C5F-D0E3-4554-8BDA-CA218B10DCEF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0584 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-616D0F96-C788-4741-A2A4-5982232A07D3.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0585 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-D1A49E8D-C748-447F-A652-5526B3C78736.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0586 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-759D5040-BB36-43B0-A646-830139451CCD.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0587 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-7D7C1D92-5318-4BF9-99C2-10A45113E63F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0588 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-D9AB5E1D-2D76-4D92-8000-24B78AAF09CC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0589 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-GetStarted/files/GUID-8E523AD5-4CB6-4239-946F-0FC5249CE6EC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0590 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-D9EDB8DB-2D08-40B2-9979-2F1A5B5962F6.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0591 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-4460B588-3FF6-4AA5-9E35-2E452E7070BA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0592 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-65EA44EC-C777-4C97-8718-5783C3A80B0A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0593 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-514B6D3E-9486-4EF4-81B1-F544D527E708.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0594 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-F9DFEDF3-FA2C-4A9D-80A4-DDDA1898A0E4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0595 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-2B21C6A4-203C-4F85-B613-D2432AD95595.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0596 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-DA150C6B-996C-4C70-9E8C-3C536C232851.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0597 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-924CBB1E-71D9-47A4-9463-89CCB680C633.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0598 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-6C1A6D69-24BA-4A2A-91C6-8DB75884A0F4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0599 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-A321428F-96FC-4CA8-AA65-846401353BF9.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0600 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-CD4D9383-956F-4A6C-9158-41DF6AC36BAE.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0601 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-EDD5CAD0-B731-4876-B028-32DC057E7416.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0602 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-2483AA02-B0F0-40E3-9194-AE21EA59358D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0603 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-302EDDD4-37BD-4CEA-97F9-1ADA833B509E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0604 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-94411536-561B-448D-A8F8-A054C90EB4D1.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0605 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-37A4B924-AB44-486F-AD27-02F414DE4683.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0606 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-FF744EF9-336A-4618-A80E-F91866359C68.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0607 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-0A271E99-7014-4705-83AF-8A7188E22134.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0608 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-09956850-F036-49C7-81DC-B9296ACF53BA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0609 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-22B6D041-F6D8-4DB0-8094-B325CC3CC6B4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0610 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-50FB6EAF-5308-487B-9BF0-A59C36126B96.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0611 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-01E2AE57-7DDE-4A62-892B-522945D5DE36.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0612 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2016/ENU/Revit-Model/files/GUID-137B997D-15A7-48CC-9377-744A511D6A6C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0613 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-F6AD75EF-5D85-4588-8C1D-0E0092319A0C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0614 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-485FC088-6889-48C0-A549-2524D06E0A24.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0615 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-5803796F-3FE7-4989-B3C5-4A880FA9028B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0616 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-F56B56F0-E729-4A8B-A4CD-9664EF0EAC6D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0617 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-332C00ED-8157-495E-9EF9-250B32E4A380.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0618 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-E4F66BE4-88B7-46D3-8BDB-F2BFBBA3A912.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0619 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-WhatsNew/files/GUID-74B97903-B4C9-4293-9C15-F952C89D0BBA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0620 | ARC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-0DB74A8A-545C-4F5A-A8A4-FCB78AA93BDE.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0621 | ARC | tutorial / reseller / press / data provider | https://resources.ascented.com/revit/what-s-new-in-revit-site-planning-and-design-2024 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0622 | MEP/ANA | vendor-help | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-MEPEng/files/GUID-01E59ECD-17C5-4390-87AE-92588A3FD9C3.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0623 | MEP/ANA | vendor-help | https://help.autodesk.com/cloudhelp/2014/ENU/Revit/files/GUID-07845892-5B60-4F66-8E44-9E6D713FCB3A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0624 | MEP/ANA | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-MEPEng/files/GUID-9C0646E1-02B7-4880-A065-BEDF6078D84A.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0625 | MEP/ANA | vendor-help | https://help.autodesk.com/cloudhelp/2014/ENU/Revit/files/GUID-E8AC04D7-79A2-485A-B1AA-4F0775A57EE9.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0626 | MEP/ANA | vendor-help | https://help.autodesk.com/cloudhelp/2014/ENU/Revit/files/GUID-831C99C4-1DA7-4385-A686-F69BAF0115A7.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0627 | MEP/ANA | tutorial / reseller / press / data provider | https://ukcommunity.arkance.world/hc/en-us/articles/21566045395346-Revit-2022-Enhanced-Feature-People-flow-Toolkit | 403 | PASS: public page; automated access blocked or rate-limited (bot protection), no login required |
| S0628 | MEP/ANA | tutorial / reseller / press / data provider | https://www.oekobaudat.de/en | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0629 | MEP/ANA | tutorial / reseller / press / data provider | https://circularecology.com/ice-database-faqs.html | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0630 | MEP/ANA | vendor-blog | https://www.autodesk.com/blogs/aec/2026/04/07/introducing-forma-carbon-insights-collaborative-total-carbon-analysis/ | 403 | PASS: public page; automated access blocked or rate-limited (bot protection), no login required |
| S0631 | MEP/ANA | open-standard / open-source | https://github.com/DavidVeld/CarboLifeCalc | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0632 | STR | vendor-help | https://help.autodesk.com/cloudhelp/2014/ENU/Revit/files/GUID-C1386ECF-C731-4AA9-B7F9-CD69E0B9C8DC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0633 | STR | vendor-help | https://help.autodesk.com/cloudhelp/2014/ENU/Revit/files/GUID-6C22A406-A8D5-49D3-8594-AA9226FD147B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0634 | STR | vendor-help | https://help.autodesk.com/cloudhelp/2014/ENU/Revit/files/GUID-2EBFF6A4-0BA9-486B-9CA2-A53F4DE36003.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0635 | STR | vendor-help | https://help.autodesk.com/cloudhelp/2019/ENU/Revit-Analyze/files/GUID-EDC6D8B8-AA46-487F-924D-4A31A3B71736.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0636 | STR | tutorial / reseller / press / data provider | https://www.lynda.com/Revit-Structure-tutorials/Configuring-connection-settings/5019798/3507066-4.html | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0637 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-EBB5B040-BEA8-4867-B245-8C1308EE3AE4.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0638 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-B958E9F3-3B5B-464A-9BB4-045120DF312E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0639 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-361CD6B7-38FA-47B3-86AA-ECF8BD0F8F40.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0640 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-0191EE82-E7A3-43EE-9BB9-B8A4D8CC4721.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0641 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-StructEng/files/GUID-BA455C5D-A608-4BA9-97F8-2D7827C6C785.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0642 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-CEF1A80C-C184-4FC1-8679-D211BE2361E9.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0643 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-ArchDesign/files/GUID-4FF3E72D-50B7-4C5A-AB09-1999CF901F9B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0644 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-7ECEE8CB-2BDD-4279-963F-DDD776A28D74.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0645 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-20E41D23-4B3E-4BEA-A114-89CBBC3F88E2.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0646 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Model/files/GUID-BE871A38-C99F-4D07-93B5-576CEBA0E68D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0647 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-6008BE14-4272-4F55-A985-F8C86C7F93E5.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0648 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-1423DE8C-8686-4D17-8B99-BF876F522915.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0649 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-4FE91053-5923-43D3-9A15-C911173B3BBB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0650 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-A8B2C17A-2252-4474-B290-494BE45D1C24.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0651 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-EC855450-8592-494F-AF7C-903ACD916A74.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0652 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-1F177E3C-6C6B-4D0D-BD5B-D1904EB3120D.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0653 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-ADFB8EA4-191A-48B7-ACE3-436F89FB68EF.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0654 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Analyze/files/GUID-1074AB24-51A4-43BE-816B-BB2231D5C988.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0655 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-60295DA1-688A-4FB9-8CF4-3F336D0D2022.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0656 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-2FD241CF-EBFB-48FC-BF1B-77A85912489F.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0657 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-1ADBC3F0-3779-44C4-ABE5-85447146CDCA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0658 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-BEEAE120-5B0F-45C1-AA63-5771B886800C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0659 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-596A36AD-686F-4299-8FB1-A851975AB2DB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0660 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-745313E6-93F6-4FA2-A7D0-7D45242F5520.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0661 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-3CBE91F5-985D-412A-84F2-CDD712CFC553.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0662 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-448E4546-7333-4051-B673-4D7652EAD2E3.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0663 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-5E76AF3A-00DB-4D4E-8C4F-780678B7E5DB.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0664 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-8DC49C5E-E0B0-41A1-8933-B62755A2FA07.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0665 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-246707F0-6ECB-42F3-9044-511067569C2E.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0666 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-2D29E602-A8F5-4DA5-8567-4934FDA27C36.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0667 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-Customize/files/GUID-F465D744-05F8-4A01-9151-66EF76F6107B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0668 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-AE12D8E6-E59D-4981-8842-04E772A63AAC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0669 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-E6167D97-8189-446A-B041-F4B2432C22E9.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0670 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-BB79FFE6-7CDD-46BC-80DE-22B8888097FC.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0671 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-0C893628-9560-4913-8A44-2E61DD3ACBC0.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0672 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-C5FE8198-14EF-4568-B3DE-EE975CBEE2D8.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0673 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-7FD921FB-4B58-4D8B-9F43-545657447220.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0674 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-4E9F0CC4-5447-46A1-B291-F1D93F72DE4C.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0675 | DOC | vendor-help | https://help.autodesk.com/cloudhelp/2027/ENU/Revit-DocumentPresent/files/GUID-3C81BBFC-AC75-40CB-980E-023BB27428B0.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0676 | DOC | tutorial / reseller / press / data provider | https://raw.githubusercontent.com/buildingSMART/IFC4.3.x-development/master/docs/schemas/core/IfcProductExtension/Entities/IfcElementAssembly.md | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0677 | DOC | tutorial / reseller / press / data provider | https://raw.githubusercontent.com/buildingSMART/IFC4.3.x-development/master/docs/schemas/core/IfcProductExtension/Entities/IfcElementAssemblyType.md | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0678 | DOC | tutorial / reseller / press / data provider | https://raw.githubusercontent.com/buildingSMART/IFC4.3.x-development/master/docs/schemas/core/IfcProductExtension/Types/IfcElementAssemblyTypeEnum.md | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0679 | DOC | tutorial / reseller / press / data provider | https://raw.githubusercontent.com/buildingSMART/IFC4.3.x-development/master/docs/schemas/core/IfcProductExtension/Types/IfcAssemblyPlaceEnum.md | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0680 | COL | vendor-help | https://help.autodesk.com/cloudhelp/2025/ENU/Revit-Collaborate/files/GUID-F1F4F05D-817C-48AA-A13C-C07C820C41DA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0681 | COL | vendor-help | https://help.autodesk.com/cloudhelp/2025/ENU/Revit-Collaborate/files/GUID-CE73C639-A93D-4388-9885-ECA5806551C2.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0682 | COL | vendor-help | https://help.autodesk.com/cloudhelp/2023/ENU/Revit-Collaborate/files/GUID-2593470B-FBC9-49CC-9962-664D508B2B91.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0683 | COL | vendor-help | https://help.autodesk.com/cloudhelp/2022/ENU/Revit-Collaborate/files/GUID-17AB868B-03C9-473F-9951-03BE6A947968.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0684 | COL | vendor-help | https://help.autodesk.com/cloudhelp/2025/ENU/Revit-Collaborate/files/GUID-AC3ADD25-CDBB-42BB-9C08-CED25E647B2B.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0685 | COL | vendor-help | https://help.autodesk.com/cloudhelp/2014/ENU/Revit/files/GUID-FAE15E6C-81A8-42A2-A7F1-3FD22A362EDA.htm | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0686 | ADV | standards-body blog | https://www.khronos.org/blog/godot-openxr-integration-project-update-major-openxr-features-now-production-ready | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0687 | ADV | press | https://vr.org/articles/godot-4-7-xr-steam-frame-android-xr-open-source-2026 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0688 | ADV | press | https://vr.org/articles/godot-openxr-vendors-plugin-v5-1-android-xr-may-2026 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0689 | ADV | press | https://vr.org/articles/godot-46-xr-engine-meta-funding | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0690 | ADV | press | https://uploadvr.com/godot-now-supports-more-xr-features-builds-a-universal-openxr-apk | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0691 | ADV | open-source project docs | https://godotengine.org/article/godot-xr-update-oct-2024/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0692 | ADV | open-source project docs | https://godotengine.org/article/godot-openxr-vendors-plugin-400/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0693 | ADV | press | https://www.roadtovr.com/openxr-update-godot-unity-game-engine/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0694 | ADV | press | https://alternativeto.net/news/2026/1/godot-4-6-debuts-modern-theme-jolt-physics-node-ids-and-libgodot-embed | 403 | PASS: public page; automated access blocked or rate-limited (bot protection), no login required |
| S0695 | ADV | press | https://www.phoronix.com/news/Godot-4.6-Released | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0696 | ADV | open-source project docs | https://godotengine.org/article/dev-snapshot-godot-4-6-dev-2/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0697 | ADV | forum | https://forum.godotengine.org/t/does-godot-4-support-ar-for-android-and-ios/88658 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0698 | ADV | press | https://www.roadtovr.com/?p=172858 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0699 | COL | open-standard / open-source | https://github.com/buildingSMART/BCF-API | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0700 | COL | open-standard / open-source | https://github.com/buildingSMART/foundation-API | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0701 | COL | open-standard / open-source | https://github.com/buildingSMART/documents-API | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0702 | DOC | standards-body | https://nibs.org/nbims/v3/cobie/4-2/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0703 | ARC | public guidance (municipal) | https://www.tfid.org/DocumentCenter/View/6771/2018-RESIDENTIAL-STAIR-GUIDE | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0704 | ARC | public guidance (code training) | https://buildingcodetrainer.com/commercial-stair-codes-explained/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0705 | ARC | government (UK) | https://assets.publishing.service.gov.uk/government/uploads/system/uploads/attachment_data/file/3011/Building_regulations_appeal_45-3-161.pdf | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0706 | ARC | government (Canada, NRC) | https://codes-guides.nrc.ca/IA/05NBC/nbc05ei005129.html | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0707 | ARC | government (Australia, ABCB) | https://ncc.abcb.gov.au/editions/ncc-2022/adopted/housing-provisions/11-safe-movement-and-access/part-112-stairway-and-ramp-construction | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0708 | ARC | public guidance (building inspection) | https://surebuildinginspections.au/defects/stair-headroom-insufficient/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0709 | ARC | public guidance (trade reference) | https://www.baunetzwissen.de/glossar/d/durchgangshoehe-10204364 | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0710 | ASSET | public-domain 3D model (CC0), redistributed in test-data/library | https://commons.wikimedia.org/wiki/File:Antike_griechische_S%C3%A4ule_20220605.stl | 200 | PASS: public (HTTP 200, unauthenticated); CC0 1.0 dedication by its author, no attribution required |
| S0711 | STR | encyclopedia (OpenSees licence summary) | https://en.wikipedia.org/wiki/OpenSees | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0712 | STR | open-source project documentation (OpenSees licence) | https://openseesdocumentation.readthedocs.io/en/latest/developer/license.html | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0713 | STR | open-source project repository (PyNite, MIT) | https://github.com/JWock82/PyNite | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0714 | STR | open-source project site (Frame3DD, GPL-3.0) | https://frame3dd.sourceforge.net/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0715 | STR | open-source project repository (IfcOpenShell ifc2ca) | https://github.com/IfcOpenShell/IfcOpenShell/tree/v0.8.0/src/ifc2ca | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0716 | STR | university lab page (xara, BSD-2-Clause OpenSees fork) | https://stairlab.berkeley.edu/software/opensees | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0717 | STR | open-source project site (xara) | https://xara.so | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0718 | STR | open-source project documentation (Kratos StructuralMechanicsApplication licence) | https://kratos-docs.onrender.com/da/d9a/structural__mechanics__application_8h_source.html | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0719 | STR | open-source project site (CalculiX, GPL) | https://dhondt.de/ | 200 | PASS: public (HTTP 200, unauthenticated) |
| S0720 | STR | AISC Shapes Database v16.0 (xlsx): All AISC catalogs (W, HSS, Pipe, C, L, WT) | https://cloud.aisc.org/biggie_bin/aisc-shapes-database-v160-2.xlsx | 200 | PASS: public (no login); Spreadsheet has a disclaimer only (no licence). AISC site T&Cs (T101) restrict copying/derivative works; dimensions are facts (see brief section g) |
| S0721 | STR | AISC Shapes Database v16.0 page: Download link, description (free, US + metric units, EDI naming) | https://www.aisc.org/aisc/publications/steel-construction-manual/aisc-shapes-database-v160/ | 200 | PASS: public (no login; blocks requests without a browser user agent); Bot filter, not a login. AISC T&Cs (T101) |
| S0722 | STR | ArcelorMittal Sections & Merchant Bars sales programme V2026-1 (PDF): IPE, HEA, HEB data; UB/UC and metric W cross-check; EN 10025-2 and ASTM A36/A572/A992 property tables | https://sections.arcelormittal.com/repo/Sections/Sections_MB_ArcelorMittal_FR_EN_DE_V2026-1.pdf | 200 | PASS: public (no login; blocks requests without a browser user agent); Linked from https://sections.arcelormittal.com/products_and_solutions/Product_catalogues/EN. Terms (T105): no copying/distribution without consent; personal use |
| S0723 | STR | eurocodeapplied.com: IPE/HEA/HEB/HEM design properties: Second source for IPE/HEA/HEB | https://eurocodeapplied.com/design/en1993/ipe-hea-heb-hem-design-properties | 200 | PASS: public (no login); Terms (T99): no republishing without permission. Used for verification only |
| S0724 | STR | SCI/BCSA Steel for Life Blue Book: UB dimensions & properties (EC3 UK NA): UB catalog (table loaded from /umbraco/surface/datatable/DataTable_SecPropsDimsProps with xmlfile ec3_properties_ub_uk-v18.xml, pageId 1126) | https://www.steelforlifebluebook.co.uk/ub/ec3-ukna/section-properties-dimensions-properties/ | 200 | PASS: public (no login); Footer: (c) SCI, all rights reserved; no terms page (T104) |
| S0725 | STR | SCI/BCSA Steel for Life Blue Book: UC dimensions & properties (EC3 UK NA): UC catalog (same endpoint, ec3_properties_uc_uk-v18.xml, pageId 1153) | https://www.steelforlifebluebook.co.uk/uc/ec3-ukna/section-properties-dimensions-properties/ | 200 | PASS: public (no login); As T05 |
| S0726 | STR | Atlas Tube: HSS section properties, A500 squares (PDF): Cross-check of AISC square HSS | https://www.atlastube.com/wp-content/uploads/2018/04/A500-Square-Current.pdf | 200 | PASS: public (no login); Manufacturer catalogue |
| S0727 | STR | Engineering Toolbox: ANSI/ASME steel pipe dimensions: Cross-check of Pipe STD sample | https://www.engineeringtoolbox.com/ansi-steel-pipes-d_305.html | 200 | PASS: public (no login); Reference site |
| S0728 | STR | eurocodeapplied.com: concrete properties table (EN 1992-1-1): EC2 class values, nu, alpha | https://eurocodeapplied.com/design/en1992/concrete-design-properties | 200 | PASS: public (no login); Terms (T99) |
| S0729 | STR | civils.ai: Eurocode concrete design properties table: Cross-check of EC2 class values | https://civils.ai/blog/table-of-eurocode-3-concrete-design-properties | 200 | PASS: public (no login);  |
| S0730 | STR | New Steel Construction: 'They've changed the density of concrete': EN 1991-1-1 concrete unit weight 24 (+1) kN/m3 | https://www.newsteelconstruction.com/wp/theyve-changed-the-density-of-concrete-or-have-they/ | 200 | PASS: public (no login);  |
| S0731 | STR | LibreTexts: Types of structural loads (dead load table): Concrete 145/150 pcf | https://eng.libretexts.org/Bookshelves/Mechanical_Engineering/Introduction_to_Aerospace_Structures_and_Materials_(Alderliesten)/01:_Introduction_to_Structural_Analysis_and_Structural_Loads/02:_Structural_Loads_and_Loading_System/2.01:_Types_of_Structural_Loads | 200 | PASS: public (no login); CC BY-NC-SA 4.0 |
| S0732 | STR | FHWA-HRT-05-057 Appendix C: Concrete unit weight range, thermal coeff 6e-6/F | https://www.fhwa.dot.gov/publications/research/infrastructure/bridge/05057/appc.cfm | 200 | PASS: public (no login); US government work |
| S0733 | STR | Penn State CE584: modulus of elasticity equations: Ec = 33 wc^1.5 sqrt(f'c) | https://engr.psu.edu/ce/courses/ce584/concrete/library/concreteprop/hpcmodelasticity/equations.html | 200 | PASS: public (no login);  |
| S0734 | STR | Oasys AdSec reference: ACI: Ec = 57000 sqrt(f'c) psi | https://docs.oasys-software.com/structural/adsec/references/aci/ | 200 | PASS: public (no login); Vendor documentation |
| S0735 | STR | Concrete Network: compressive strength (psi): Typical f'c by use | https://www.concretenetwork.com/concrete/compressive-strength-psi.html | 200 | PASS: public (no login); Trade site |
| S0736 | STR | ideCAD help: column reinforcement detailing: ACI 318-19 Table 20.5.1.3.1 cover values | https://help.idecad.com/ideCAD/column-reinforcement-detailing | 200 | PASS: public (no login); Vendor documentation |
| S0737 | STR | ACI FAQ 903: 3 in cover cast against earth (ACI 318-19) | https://www.concrete.org/frequentlyaskedquestions.aspx?faqid=903 | 200 | PASS: public (no login); Publisher page |
| S0738 | STR | FRILO: Durability, creep coefficient and shrinkage strain (PDF): EC2 Table 4.4N cmin,dur, Δcdev, Table E.1N | https://www.frilo.eu/wp-content/uploads/EN/Manuals/durability_creep_coefficient_and_shrinkage_strain.pdf | 200 | PASS: public (no login); Vendor manual |
| S0739 | STR | Structville: concrete cover to Eurocode 2: cmin formula | https://structville.com/2017/11/durability-of-structures-and-how-to-calculate-concrete-cover-eurocode-2.html | 200 | PASS: public (no login);  |
| S0740 | STR | eurocodeapplied.com: EC2 concrete cover detailed calculation: S4 = 50-yr life; cmin,b = bar diameter | https://eurocodeapplied.com/design/en1992/concrete-cover/details/ec2-concrete-cover-detailed-calculation | 200 | PASS: public (no login); Terms (T99) |
| S0741 | STR | EU JRC Eurocodes: Mancini EC2 slides (PDF): Exposure class families | https://eurocodes.jrc.ec.europa.eu/sites/default/files/2022-06/D1.9Mancini.pdf | 200 | PASS: public (no login); Official EU site |
| S0742 | STR | ToolGrit: concrete column guide: ACI has no minimum column dimension; practical minimums | https://www.toolgrit.com/guides/concrete-column-guide | 200 | PASS: public (no login); Secondary source |
| S0743 | STR | StructurePoint: ACI 318-19 column slenderness flowchart (PDF): klu/r limits 22 / 34+12(M1/M2) <= 40 | https://structurepoint.org/pdfs/08_Concrete_Column_Slenderness%20Flowchart_Equations_ACI%20318-19.pdf | 200 | PASS: public (no login); Vendor |
| S0744 | STR | Structville: detailing of columns to Eurocode 2: EC2 9.5.2 bar rules | https://structville.com/2020/04/detailing-of-columns-to-eurocode-2.html | 200 | PASS: public (no login);  |
| S0745 | STR | Dlubal KB 001710: EC2 5.8.3.1 λlim | https://www.dlubal.com/en/support-and-learning/support/knowledge-base/001710 | 200 | PASS: public (no login); Vendor |
| S0746 | STR | SCIA help: limit slenderness: λlim is an NDP | https://help.scia.net/19.1/en/val/concrete15/slendernesstb/slenderness_04_calculation_limit_slenderness.htm | 200 | PASS: public (no login); Vendor |
| S0747 | STR | NIST: PS 20-20 American Softwood Lumber Standard: Lumber nominal/dressed sizes, definitions | https://www.nist.gov/system/files/documents/2019/12/11/PS%2020-20%20final%20WERB%20approved.pdf | 200 | PASS: public (no login); US government publication |
| S0748 | STR | AWC 2024 NDS Supplement: section properties (PDF): Cross-check of lumber sizes | https://web-media.awc.org/wp-content/uploads/2021/10/17210145/AWC_NDS2024-Supplement_20231129_AWCWebsite_Section-Properties.pdf | 200 | PASS: public (no login); AWC copyright; cite facts only |
| S0749 | STR | AWC 2024 NDS page (free view-only): NDS free view-only access | https://awc.org/publications/2024-nds | 200 | PASS: public (no login); View-only |
| S0750 | STR | AWC 2024 NDS Supplement Ch.4 reference design values (PDF): Table 4A DF-L No.2, SPF No.1/No.2 | https://awc.org/wp-content/uploads/2026/08/AWC_NDS2024-Supplement_20240719_Chapter-4-Reference-Design-Values_Website-1.pdf | 200 | PASS: public (no login); Intended view-only; cite values, do not redistribute |
| S0751 | STR | SkyCiv BeamDimensions: NDS Table 4A: Cross-check Table 4A | https://beamdimensions.com/materials/Wood/NDS/Table_4A | 200 | PASS: public (no login); (c) SkyCiv; mislabels G |
| S0752 | STR | Studio Madera: EN 338:2016 softwood classes: EN 338 C16/C24/C30 | https://www.studiomadera.it/news/660-legno-conifera | 200 | PASS: public (no login);  |
| S0753 | STR | Frischeis: Festigkeitsklassen Schnittholz (PDF): Cross-check EN 338 | https://www.frischeis.at/pim/10%20Holzbau/datenblaetter/Holzbau/Festigkeitsklassen%20Schnittholz.pdf | 200 | PASS: public (no login); Merchant data sheet |
| S0754 | STR | Edinburgh Napier CWST: C and T classes: EN 338 2009 vs 2016 note | https://blogs.napier.ac.uk/cwst/c-and-t-classes/ | 200 | PASS: public (no login); University |
| S0755 | STR | StructuralBasics: glulam strength and stiffness: EN 14080 GL classes | https://www.structuralbasics.com/glulam-strength-stiffness/ | 200 | PASS: public (no login);  |
| S0756 | STR | Studiengemeinschaft Holzleimbau Glulam-Bulletin (PDF via mm-holz): Cross-check GL24c/GL28c | https://www.mm-holz.com/fileadmin/Bilder/Service/Technische_Dokumente/BSH-Merkblatt_en_stghb.pdf | 200 | PASS: public (no login); Industry association |
| S0757 | STR | Edinburgh Napier CWST: EN 336 and UK customary sizes: European/UK sawn sizes, tolerances | https://blogs.napier.ac.uk/cwst/en336-customary-sizes/ | 200 | PASS: public (no login); University |
| S0758 | STR | Swedish Wood: sawn timber range: Names Nordic size standards only (no table) | https://www.swedishwood.com/wood-facts/about-wood/wood-grades/sawn_timber_range/ | 200 | PASS: public (no login);  |
| S0759 | STR | ASCE 7 page: ASCE/SEI 7-22 scope; paid | https://www.asce.org/asce-7 | 200 | PASS: public (no login);  |
| S0760 | STR | AISC 360 page: AISC 360-22 | https://www.aisc.org/aisc/publications/current-standards/aisc-360/ | 200 | PASS: public (no login); Free PDF not confirmed (store widget) |
| S0761 | STR | AISC news: 360-22 released: AISC 360-22 edition | https://www.aisc.org/modern-steel/news/aisc-releases-new-version-of-specification-for-structural-steel-buildings-ansiaisc-360-22/ | 200 | PASS: public (no login);  |
| S0762 | STR | AISC standards under public review: Public-review drafts | https://www.aisc.org/aisc/publications/standards-under-public-review/ | 200 | PASS: public (no login);  |
| S0763 | STR | AISC 360-27 public-review draft (PDF, 11 Sep 2026): E, G, nu, slenderness user notes, HSS design wall | https://www.aisc.org/media/ravjq5r3/aisc-360-27_public-review_9-11-2026.pdf | 200 | PASS: public (no login); Copyright, all rights reserved: facts only |
| S0764 | STR | ICC course: structural changes to the 2024 IBC: Standards referenced by IBC 2024 | https://shop.iccsafe.org/part-1-an-overview-of-the-structural-changes-to-the-2024-ibc-recording.html | 200 | PASS: public (no login);  |
| S0765 | STR | ACI 318 building code portal: ACI 318-25 / 318-19 | https://www.concrete.org/topicsinconcrete/318buildingcodeportal.aspx | 200 | PASS: public (no login); Paid standard |
| S0766 | STR | ICC: where can I view the codes for free: ICC free read-only access | https://support.iccsafe.org/ht_kb/codes-faq/where-can-i-view-the-codes-for-free | 200 | PASS: public (no login);  |
| S0767 | STR | IBC 2024 Ch.16 Structural Design: Chapter mapping | https://codes.iccsafe.org/content/IBC2024P1/chapter-16-structural-design | 200 | PASS: public (no login); Read-only |
| S0768 | STR | IBC 2024 Ch.18 Soils and Foundations: Chapter mapping | https://codes.iccsafe.org/content/IBC2024P1/chapter-18-soils-and-foundations | 200 | PASS: public (no login); Read-only |
| S0769 | STR | IBC 2024 Ch.19 Concrete: Chapter mapping | https://codes.iccsafe.org/content/IBC2024P1/chapter-19-concrete | 200 | PASS: public (no login); Read-only |
| S0770 | STR | IBC 2024 Ch.22 Steel: Chapter mapping | https://codes.iccsafe.org/content/IBC2024P1/chapter-22-steel | 200 | PASS: public (no login); Read-only |
| S0771 | STR | IBC 2024 Ch.23 Wood: Chapter mapping | https://codes.iccsafe.org/content/IBC2024P1/chapter-23-wood | 200 | PASS: public (no login); Read-only |
| S0772 | STR | IRC 2024 Ch.5 Floors: Chapter mapping | https://codes.iccsafe.org/content/IRC2024P1/chapter-5-floors | 200 | PASS: public (no login); Read-only |
| S0773 | STR | IRC 2024 Ch.6 Wall Construction: Chapter mapping | https://codes.iccsafe.org/content/IRC2024P1/chapter-6-wall-construction | 200 | PASS: public (no login); Read-only |
| S0774 | STR | IRC 2024 Ch.8 Roof-Ceiling Construction: Chapter mapping | https://codes.iccsafe.org/content/IRC2024P1/chapter-8-roof-ceiling-construction | 200 | PASS: public (no login); Read-only |
| S0775 | STR | NRC: National Building Code of Canada 2020: NBC 2020, free electronic | https://nrc.canada.ca/en/certifications-evaluations-standards/codes-canada/codes-canada-publications/national-building-code-canada-2020 | 200 | PASS: public (no login); Free |
| S0776 | STR | NRC: National Building Code of Canada 2025: NBC 2025 | https://nrc.canada.ca/en/certifications-evaluations-standards/codes-canada/codes-canada-publications/national-building-code-canada-2025 | 200 | PASS: public (no login); Free electronic |
| S0777 | STR | CSA S16:24: Canadian steel design | https://www.csagroup.org/store/product/CSA_S16:24/ | 200 | PASS: public (no login); Text needs free account (login-gated) |
| S0778 | STR | CSA A23.3:24: Canadian concrete design | https://www.csagroup.org/store/product/CSA_A23.3:24/ | 200 | PASS: public (no login); Login-gated view access |
| S0779 | STR | CSA O86:24: Canadian wood design | https://www.csagroup.org/store/product/CSA_O86:24/ | 200 | PASS: public (no login); Login-gated view access |
| S0780 | STR | EU JRC: Eurocodes family: EN 1990-1999 scopes | https://eurocodes.jrc.ec.europa.eu/en-eurocodes/eurocodes-family | 200 | PASS: public (no login); Official EU |
| S0781 | STR | EU JRC: road to second-generation Eurocodes: 2nd-gen timeline | https://eurocodes.jrc.ec.europa.eu/2nd-generation/road-second-generation-eurocodes-where-are-we | 200 | PASS: public (no login);  |
| S0782 | STR | EU JRC: second-generation milestones: Withdrawal date 2028 | https://eurocodes.jrc.ec.europa.eu/news/second-generation-eurocodes-milestones-achieved | 200 | PASS: public (no login);  |
| S0783 | STR | EU JRC: nationally determined parameters: NDP concept | https://eurocodes.jrc.ec.europa.eu/en-eurocodes-implementation/nationally-determined-parameters | 200 | PASS: public (no login);  |
| S0784 | STR | EU JRC: national implementation: National Annexes | https://eurocodes.jrc.ec.europa.eu/en-eurocodes/eurocodes-national-implementation | 200 | PASS: public (no login);  |
| S0785 | STR | SteelConstruction.info: design codes and standards: UK Eurocodes + NAs; BS 5950 withdrawn 2010 | https://steelconstruction.info/topics/design/design-codes-and-standards/ | 200 | PASS: public (no login);  |
| S0786 | STR | NSC: British Standards to be withdrawn: March 2010 withdrawal | https://www.newsteelconstruction.com/wp/british-standards-to-be-withdrawn-in-favour-of-eurocodes/ | 200 | PASS: public (no login);  |
| S0787 | STR | BSI Knowledge: BS 8110-1:1997: BS 8110 withdrawn 2010 | https://knowledge.bsigroup.com/products/structural-use-of-concrete-code-of-practice-for-design-and-construction | 200 | PASS: public (no login);  |
| S0788 | STR | gov.uk: Approved Document A (Structure): UK Building Regs structure guidance (free) | https://www.gov.uk/government/publications/structure-approved-document-a | 200 | PASS: public (no login); Crown copyright / OGL |
| S0789 | STR | ABCB NCC online: Australian NCC free online | https://ncc.abcb.gov.au/ | 200 | PASS: public (no login);  |
| S0790 | STR | NCC 2025 copyright/licence notice: NCC under CC BY 4.0 | https://ncc.abcb.gov.au/editions/ncc-2025/adopted/volume-one/preface/copyright-licence-notice-and-acknowledgment-country | 200 | PASS: public (no login); CC BY 4.0 (excl. third-party) |
| S0791 | STR | Standards Australia store: AS/NZS 1170.0:2002: Edition/status | https://store.standards.org.au/product/as-nzs-1170-0-2002 | 200 | PASS: public (no login); Paid |
| S0792 | STR | Standards Australia store: AS 4100:2020: Edition/status | https://store.standards.org.au/product/as-4100-2020 | 200 | PASS: public (no login); Paid |
| S0793 | STR | Standards Australia store: AS 3600:2018: Edition/status | https://store.standards.org.au/product/as-3600-2018 | 200 | PASS: public (no login); Paid |
| S0794 | STR | Standards Australia store: AS 1720.1-2010: Edition/status | https://store.standards.org.au/product/as-1720-1-2010 | 200 | PASS: public (no login); Paid |
| S0795 | STR | Standards Australia store: AS 1684.2:2021: Edition/status | https://store.standards.org.au/product/as-1684-2-2021 | 200 | PASS: public (no login); Paid |
| S0796 | STR | Standards Australia store: AS/NZS 3679.1:2016: Edition/status | https://store.standards.org.au/product/as-nzs-3679-1-2016 | 200 | PASS: public (no login); Paid |
| S0797 | STR | Standards Australia store: AS/NZS 1170.2:2021: Edition/status | https://store.standards.org.au/product/as-nzs-1170-2-2021 | 200 | PASS: public (no login); Paid |
| S0798 | STR | Standards Australia store: AS 1170.4:2024: Edition/status | https://store.standards.org.au/product/as-1170-4-2024 | 200 | PASS: public (no login); Paid |
| S0799 | STR | MBIE: B1/VM1 Second Edition (PDF): NZ cited structural standards | https://www.building.govt.nz/assets/Uploads/building-code-compliance/b-stability/b1-structure/asvm/b1-structure-vm1-second-edition.pdf | 200 | PASS: public (no login); Free |
| S0800 | STR | Standards NZ: sponsored building-related standards: Free NZ standards list (NZS 3101, 3404, 1170.5, 3604) | https://www.standards.govt.nz/get-standards/sponsored-standards/building-related-standards | 200 | PASS: public (no login); Free download (sponsored) |
| S0801 | STR | AISC Modern Steel Construction (June 2022): 'Are You Properly Specifying Materials?': Preferred US grades and Fy/Fu | https://lsc-pagepro.mydigitalpublication.com/article/Are+You+Properly+Specifying+Materials%3F/4268586/746763/article.html | 200 | PASS: public (no login);  |
| S0802 | STR | CISC: steel products today: Canada W = A992, HSS = G40.21 350W | https://www.cisc-icca.ca/steel-products-today/ | 200 | PASS: public (no login);  |
| S0803 | STR | CISC: A500 vs G40.21 HSS: Not an exact substitution | https://www.cisc-icca.ca/?p=14670 | 200 | PASS: public (no login);  |
| S0804 | STR | Atlas Tube: comparison of A500, A252, G40.21 (PDF): 350W Fy/Fu | https://www.atlastube.com/wp-content/uploads/2018/03/Comparison-of-ASTM-A500-ASTM-A252-CSA-G40.21.pdf | 200 | PASS: public (no login); Manufacturer |
| S0805 | STR | SteelConstruction.info: steel construction products: EN 10365, EN 10210-2, EN 10219-2 | https://www.steelconstruction.info/Steel_construction_products | 200 | PASS: public (no login);  |
| S0806 | STR | SteelConstruction.info: steel material properties: E, G, nu, alpha; common UK grades | https://www.steelconstruction.info/Steel_material_properties | 200 | PASS: public (no login);  |
| S0807 | STR | InfraBuild: universal beams: 300PLUS to AS/NZS 3679.1 | https://www.infrabuild.com/steel/products/structural-steel/structural-steel-universal-beams/ | 200 | PASS: public (no login); Manufacturer |
| S0808 | STR | Australian Steel Institute: long products made in Australia: 300PLUS meaning | https://steel.org.au/about-us/our-industry/steel-long-products-made-in-australia/ | 200 | PASS: public (no login);  |
| S0809 | STR | Edcon Steel: 300PLUS data sheet (PDF): 300PLUS Fy/Fu | https://www.edconsteel.com.au/store/storage/pdf/300plus.pdf | 200 | PASS: public (no login); Distributor |
| S0810 | STR | Cambridge University Press excerpt (Australian concrete design): AS 3600 strength grades | https://assets.cambridge.org/97811084/05645/excerpt/9781108405645_excerpt.pdf | 200 | PASS: public (no login); Publisher excerpt |
| S0811 | STR | Steel Tube Institute: HSS ASTM article (PDF): A500-21 Gr C 50/62 all shapes; tolerances | https://steeltubeinstitute.org/wp-content/uploads/2022/08/HSS_Article_ASTM-080722.pdf | 200 | PASS: public (no login);  |
| S0812 | STR | Atlas Tube: grade comparison A252 vs A500 (PDF): A500 Gr B 46/58 | https://www.atlastube.com/wp-content/uploads/2022/03/Steel-Grade-Comparison-Flyer-ASTM-A252-vs.-ASTM-A500.pdf | 200 | PASS: public (no login); Manufacturer |
| S0813 | STR | Iowa DOT bridge design manual: Steel thermal coeff 6.5e-6/F (citing AASHTO) | https://iowadot.gov/media/4649/download | 200 | PASS: public (no login); State government |
| S0814 | STR | Stahlwerk Thüringen: structural steel grades (PDF): EN 10025-2 cross-check | https://www.stahlwerk-thueringen.de/wp-content/uploads/2025/07/Structural-steel-grades.pdf | 200 | PASS: public (no login); Mill data sheet |
| S0815 | STR | eurocodeapplied.com: steel design properties: EN 1993-1-1 Table 3.1, density | https://eurocodeapplied.com/design/en1993/steel-design-properties | 200 | PASS: public (no login); Terms (T99) |
| S0816 | STR | eurocodeapplied.com terms: Licence terms | https://eurocodeapplied.com/terms | 200 | PASS: public (no login); No republishing without permission |
| S0817 | STR | ASTM A6/A6M-24b product page: Scope of A6 | https://www.astm.org/a0006_a0006m-24b.html | 200 | PASS: public (no login); Standard is paid |
| S0818 | STR | AISC Terms & Conditions: Site licence terms | https://www.aisc.org/aisc/about-aisc/terms-conditions/ | 200 | PASS: public (no login);  |
| S0819 | STR | Cornell LII: Feist v. Rural Telephone, 499 U.S. 340 (1991): Facts not copyrightable | https://www.law.cornell.edu/supremecourt/text/499/340 | 200 | PASS: public (no login);  |
| S0820 | STR | EUR-Lex: Directive 96/9/EC (databases): Sui generis database right | https://eur-lex.europa.eu/eli/dir/1996/9/oj/eng | 200 | PASS: public (no login);  |
| S0821 | STR | Steel for Life Blue Book home: Site terms check | https://www.steelforlifebluebook.co.uk/ | 200 | PASS: public (no login); (c) SCI, no terms page |
| S0822 | STR | ArcelorMittal Sections: terms of use: Site terms | https://sections.arcelormittal.com/Terms_of_use_and_credits/EN | 200 | PASS: public (no login); Personal use; no distribution without consent |
| S0823 | STR | ideCAD Help: Load Combination and Strength Design per ASCE 7-16 §2.3 (basic and seismic strength combinations) | https://help.idecad.com/ideCAD/load-combination-and-strength-design | 200 | PASS: public (no login); equations only |
| S0824 | STR | ČSN EN 1990 (Czech edition) Annex A1, Table A1.1 recommended ψ factors for buildings (course extract, VUT Brno) | https://www.fce.vutbr.cz/KDK/bukovska.p/zatizeni002.pdf | 200 | PASS: public (no login); factor table only |
| S0825 | STR | Web search summary: ASCE 7-22 §2.3.1 basic combinations with rain R (Eq. 2.3-1 to 2.3-4), citing STRUCTURE magazine and ANSI blog pages | https://www.structuremag.org/article/asce-7-22-flat-roof-snow-load-versus-minimum-snow-load/ | 200 | PASS: search-result summary only |
| S0826 | STR | Web search summary: EN 1990 Table A1.2(B) recommended partial factors for STR (γG,sup 1.35, γG,inf 1.00, γQ 1.50), citing Febelcem and target-reliability papers | https://www.febelcem.be/fileadmin/user_upload/autres-publications/en/eurojm.pdf | 200 | PASS: search-result summary only |
| S0827 | STR | DSTV, *Standard Description for Steel Structure Pieces for the Numerical Controls*, 7th edition (July 1998), English translation: blocks ST/EN/BO/AK/IK/SI, header lines, face codes, piece coordinates, worked example | https://support.tekla.com/dist/sxf/document/dstv_nc_eng.pdf | 200 | PASS: public (no login); an open interchange standard, read for the file syntax |
| S0828 | STR | Tekla support page "DSTV file description" (block and profile code lists) | https://support.tekla.com/doc/tekla-structures/2025/int_nc_dstv_file_description | 200 | PASS: public; lists only |
| S0829 | STR | Kinetic PrimeCut help "DSTV File Format Information" (supported block list) | https://kineticusa.com/PrimecutHelp/dstv_file_format.html | 200 | PASS: public; lists only |
| S0830 | STR | Web search summary: BS 8666:2020 shape code total length formulas (11: A + (B) − 0.5r − d; 21: A + B + (C) − r − 2d; 51: 2(A + B + (C)) − 2.5r − 5d, or 2A + 2B + max(16d, 160)), citing Heaton Manufacturing, Coen Steel, Collins Reinforcements and KB Rebar pages | https://heatonmanufacturing.co.uk/rebar-shape-codes-uk-bs-8666-2005/ | 200 (search) | PASS: search-result summary only |
| S0831 | STR | KB Rebar, "BS 8666: Shape Codes" (shape descriptions for 00, 11, 21, 51, 63 and the minimum bend radius table r by bar size: 12, 16, 20, 24, 32, 70, 87, 112, 140, 175 mm for 6 … 50 mm bars) | https://kbrebar.co.uk/bs8666-shape-codes/ | 200 | PASS: public; facts of the standard (shape-code numbers, formulas, minimum radii) |

## B. Screenshots (142) — internal reference only

These images are copyrighted by their owners, mostly Autodesk, Inc. They were retrieved from publicly served URLs and are kept only as internal reference for writing the functional specification. Do not ship, publish or trace them into Reviera assets.

| ID | File | Depicts | Source URL | Status |
|----|------|---------|------------|--------|
| I001 | `screenshots/ui/dark_theme_black_canvas_2023.png` | Pre-2024 black background, light UI | https://www.cad-notes.com/wp-content/uploads/2023/04/2023-black-background.png | 200 |
| I002 | `screenshots/ui/dark_theme_full_ui.png` | Dark UI theme with dark canvas | https://www.cad-notes.com/wp-content/uploads/2023/04/working-with-selections.png | 200 |
| I003 | `screenshots/ui/dockable_windows_grouped.png` | Grouped dockable windows | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-BBEE314E-0C80-4DD5-BE63-1BC832B0BE03.png | 200 |
| I004 | `screenshots/ui/file_tab_menu.png` | File tab menu | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-1DCA4E6A-F6E9-4DE7-861B-0E66B85A9C25.png | 200 |
| I005 | `screenshots/ui/help_menu.png` | Help menu in title bar | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-D705A839-0676-4254-B8CB-8A321C9153B4.png | 200 |
| I006 | `screenshots/ui/home_classic.png` | Classic Revit Home | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-175F069E-0671-4A97-8361-8BD8EDA7994E.png | 200 |
| I007 | `screenshots/ui/home_new_projects.png` | New Revit Home, Autodesk Projects page | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-2B906D16-2648-4D2A-999B-FAC175048BF2.png | 200 |
| I008 | `screenshots/ui/home_new_recent.png` | New Revit Home, Recent page | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-BE73BD2D-2596-43C0-BFD8-FAFFD87253B2.png | 200 |
| I009 | `screenshots/ui/infocenter_search.png` | InfoCenter search box | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-433AEF79-090A-4157-BD6F-273FBA91F826.png | 200 |
| I010 | `screenshots/ui/keytips.png` | Keytips | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-B615F360-DFF4-400E-B1CA-AE9ECC000823.png | 200 |
| I011 | `screenshots/ui/multi_monitor_views.png` | Multi-monitor floating view | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-8C08D236-849B-43D6-9192-A27CF3954ACC.png | 200 |
| I012 | `screenshots/ui/navigation_bar.png` | Navigation Bar | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-F8337FFE-5D36-4C26-8B1E-DA2DE5FD48C0.png | 200 |
| I013 | `screenshots/ui/options_bar.png` | Options Bar | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-CCF99547-F6D8-4A0F-908C-49BA85BF3C49.png | 200 |
| I014 | `screenshots/ui/options_colors_tab_theme.png` | Options dialog, Colors tab | https://www.cad-notes.com/wp-content/uploads/2023/04/revit-theme-color-setting.png | 200 |
| I015 | `screenshots/ui/project_browser_search.gif` | Project Browser search (2026) | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-85856A2B-FB2C-4564-8BFE-A9C7F4C016E8.gif | 200 |
| I016 | `screenshots/ui/properties_filter_dropdown.png` | Properties filter drop-down | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-2D20DABE-BB53-478E-A0E9-DA371A9A9BE0.png | 200 |
| I017 | `screenshots/ui/properties_palette_annotated.png` | Properties palette, annotated | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-89105950-A51D-4435-A343-B51C4C1A3E00.png | 200 |
| I018 | `screenshots/ui/qat_customize_dropdown.png` | Customize QAT drop-down | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-48184962-05D5-4F67-88C5-54A872906302.png | 200 |
| I019 | `screenshots/ui/quick_access_toolbar.png` | Quick Access Toolbar | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-6B6325AD-EE34-4CE5-844F-F36EBE528A56.png | 200 |
| I020 | `screenshots/ui/revit2027_autodesk_assistant_button.png` | Autodesk Assistant button (2027) | https://www.symetri.co.uk/media/byrpyebk/interface-autodesk-assistant-revit-core.png | 200 |
| I021 | `screenshots/ui/revit2027_autodesk_assistant_panel.png` | Autodesk Assistant palette (2027) | https://www.symetri.co.uk/media/jgvpl0vh/autodesk-assistant-revit-core.png | 200 |
| I022 | `screenshots/ui/revit2027_options_bar_removed.png` | 2027: Options Bar contents relocated to contextual ribbon panels | https://www.symetri.co.uk/media/uvxcoma0/options-bar-revit-core.png | 200 |
| I023 | `screenshots/ui/ribbon_contextual_tab.png` | Ribbon with Modify / Walls contextual tab | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-5407BAF8-94D7-488D-83A5-AB3CBE2E044D.png | 200 |
| I024 | `screenshots/ui/ribbon_dialog_launcher.png` | Dialog launcher | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-ED8D7DAD-268A-48F4-B0CA-21D0C38BBE49.png | 200 |
| I025 | `screenshots/ui/ribbon_expanded_panel.png` | Expanded ribbon panel | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-865103DF-EFC6-41A5-B0BF-20E55280F3D1.png | 200 |
| I026 | `screenshots/ui/ribbon_expanded_panel_pinned.png` | Expanded panel with push-pin | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-7CDDA0CA-E99F-415F-8F15-017F4406E9FD.png | 200 |
| I027 | `screenshots/ui/ribbon_insert_tab_legacy.png` | Insert tab (older layout) | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-632AB75F-370C-4CD7-94B7-61EB4DDDAFB3.png | 200 |
| I028 | `screenshots/ui/status_bar.png` | Status bar | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-563B0990-7903-4E2A-A398-A4D658D99AA9.png | 200 |
| I029 | `screenshots/ui/steeringwheel_2d.png` | 2D Navigation Wheel | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-FFEF9999-6519-4511-B008-2BE51B343591.png | 200 |
| I030 | `screenshots/ui/steeringwheel_full_navigation.png` | Full Navigation Wheel | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-E242DA89-9973-44ED-AF74-BADBA19A361C.png | 200 |
| I031 | `screenshots/ui/tab_views.png` | Tab Views | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-8194A87C-04F4-49D2-9B95-968C700A0027.png | 200 |
| I032 | `screenshots/ui/temporary_hide_isolate.gif` | Temporary Hide/Isolate | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-962DE732-A549-44FC-9169-158078A5FF05.gif | 200 |
| I033 | `screenshots/ui/tile_views.gif` | Tile Views | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-D1C2D380-CC03-4423-B383-3F1BB515BB23.gif | 200 |
| I034 | `screenshots/ui/tooltip_ribbon.png` | Ribbon tooltip | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-E39F074A-A1D3-45EF-9853-5A53B1B131E9.png | 200 |
| I035 | `screenshots/ui/type_selector_search.png` | Type Selector search | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-EA851C30-BBFA-428B-97B3-041F19889F47.png | 200 |
| I036 | `screenshots/ui/ui_insert_tab_floor_plan.png` | Revit window with Insert tab active, floor plan open | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-2F8F40BC-B0FD-4959-B03B-120494D92665.png | 200 |
| I037 | `screenshots/ui/ui_overview_annotated.png` | Annotated overview of the Revit 2026 user interface | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-453F6BD3-DC8C-4142-9996-F7BCEB6CEB46.png | 200 |
| I038 | `screenshots/ui/view_control_bar.gif` | View Control Bar | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/images/GUID-22463280-BE9E-49ED-98D7-FBE594FD5969.gif | 200 |
| I039 | `screenshots/ui/view_tab_windows_panel_canvas_theme.png` | View tab Windows panel with Canvas Theme | https://www.cad-notes.com/wp-content/uploads/2023/04/Windows-panel.png | 200 |
| I040 | `screenshots/ui/viewcube.png` | ViewCube | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/images/GUID-FF08D2CD-F40F-4D3F-8853-C08F603DCDF0.png | 200 |
| I041 | `screenshots/architecture/compound_layer_priority_joins.png` | Layer priority joins | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-1CCD1005-CBDA-4338-8D60-489095D4BB25.htm | 200 |
| I042 | `screenshots/architecture/curtain_grid_layout_controls.png` | Curtain grid layout controls | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-E5902FF8-506E-4517-9A60-E9C6EA4422FD.htm | 200 |
| I043 | `screenshots/architecture/curtain_wall_rendered.png` | Curtain wall | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-BBC16AF1-88C1-4B2E-A23A-917C3F427E97.htm | 200 |
| I044 | `screenshots/architecture/grid_type_properties_dialog.png` | Grid type properties | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-6E5C3207-CA80-452A-9DD8-CA94CD592E51.htm | 200 |
| I045 | `screenshots/architecture/levels_in_elevation_view.png` | Levels in an elevation view | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-10A920FD-1A4C-457B-8827-D36AD7EA2D23.htm | 200 |
| I046 | `screenshots/architecture/modify_walls_contextual_ribbon.png` | Modify / Walls contextual tab | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/files/GUID-1CA04013-04CE-4F55-9B0C-68FD7E7FF80B.htm | 200 |
| I047 | `screenshots/architecture/roof_different_eave_heights_render.png` | Roof with different eave heights | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-98A54E99-6889-4BA6-8483-CE67990A49C7.htm | 200 |
| I048 | `screenshots/architecture/roof_footprint_sketch_mode.png` | Roof footprint sketch | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-GetStarted/files/GUID-5924644F-C73E-4B76-897D-B181EE9F1B16.htm | 200 |
| I049 | `screenshots/architecture/room_color_scheme_by_range.png` | Color scheme by range | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-8EE49924-9905-4E1F-9269-C819B1E1A683.htm | 200 |
| I050 | `screenshots/architecture/room_color_scheme_by_value.png` | Color scheme by value | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-8EE49924-9905-4E1F-9269-C819B1E1A683.htm | 200 |
| I051 | `screenshots/architecture/scope_boxes_in_plan.png` | Scope boxes | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-3E75F88D-D212-43D4-8176-5B7F305B60F2.htm | 200 |
| I052 | `screenshots/architecture/shape_editing_points_toposolid.png` | Shape editing points on a toposolid | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-296F9E46-85F9-4E15-9EF2-39411FA44CA3.htm | 200 |
| I053 | `screenshots/architecture/stair_direct_manipulation_controls.png` | Stair direct manipulation controls | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-0FD4C314-E5AE-4937-ABAB-8EBD95A45695.htm | 200 |
| I054 | `screenshots/architecture/stair_t_shaped_component_assembly.png` | T-shaped stair | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-D4861B15-29FF-49F8-B292-39AD62EBCDD3.htm | 200 |
| I055 | `screenshots/architecture/tapered_walls_section.png` | Tapered walls | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-BC422403-C03D-4292-AD8B-D578EC0659D1.htm | 200 |
| I056 | `screenshots/architecture/toposolid_site_plan_and_3d.png` | Toposolid | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-E2E75A78-B698-4518-A5AE-29BFF2BC2688.htm | 200 |
| I057 | `screenshots/architecture/toposolid_subdivision_road.png` | Toposolid sub-division | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/files/GUID-BA7C38A7-7E8B-45CC-B4A3-950D19B48C66.htm | 200 |
| I058 | `screenshots/architecture/wall_edit_assembly_layers_2026.png` | Edit Assembly layers grid (2026) | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-7657DF90-6AE2-4543-BFDA-7735A8424B1C.htm | 200 |
| I059 | `screenshots/architecture/wall_profile_mid_end_faces.png` | Wall profile mid-end faces | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-F3B8DD5E-5380-45C1-B0D6-E6B081224E63.htm | 200 |
| I060 | `screenshots/architecture/walls_by_room_by_segment.png` | Place By Room / Place By Segment | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/files/GUID-04D66471-BDEE-444D-BCBA-A0BDAE36032A.htm | 200 |
| I061 | `screenshots/structure_mep/analytical_automation_physical_to_analytical.png` | Physical to analytical automation result | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-BE343C51-5F71-488E-BFCA-5DB4D5CF1106 | 200 |
| I062 | `screenshots/structure_mep/analytical_duct_segment_schedule_critical_path.png` | Analytical duct segment schedule filtered by critical path | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-7367C537-C738-478C-B4EC-F9E5BBA0B8B8 | 200 |
| I063 | `screenshots/structure_mep/analytical_duct_segments_tag.png` | Analytical duct segments with tag | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-7367C537-C738-478C-B4EC-F9E5BBA0B8B8 | 200 |
| I064 | `screenshots/structure_mep/boundary_conditions_on_analytical_panel.png` | Area and point boundary conditions on an analytical panel | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-537703E1-9AA8-49E6-9835-7D779C3FD547 | 200 |
| I065 | `screenshots/structure_mep/check_pipe_systems_warning.png` | Check Pipe Systems warning marker | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-804F284A-6CE9-4311-BD2E-90748EADD5B9 | 200 |
| I066 | `screenshots/structure_mep/electrical_conductor_and_cable_settings.png` | Electrical Conductor and Cable Settings | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-2D2B6562-4997-4A80-8FFF-E0E35127A6E7 | 200 |
| I067 | `screenshots/structure_mep/energy_model_analytical_spaces_3d.png` | Energy model / analytical spaces 3D | https://help.autodesk.com/view/RVT/2027/ENU/?guid=GUID-63B2E042-CA8B-46FB-88CB-FD7ADA002973 | 200 |
| I068 | `screenshots/structure_mep/fabric_area_sheet_layout.png` | Fabric area with sheets, lap splices and tags | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-CACA6719-A8E6-4526-9171-1859315BDAA2 | 200 |
| I069 | `screenshots/structure_mep/fabrication_settings_dialog.png` | Fabrication Settings dialog | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-692C4E68-6A9D-4628-8E73-0C327E5677D4 | 200 |
| I070 | `screenshots/structure_mep/hydronic_network_primary_loop.png` | Hydronic network primary loop before separation | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-D2362C11-C604-4B32-AB76-19E6EEF75439 | 200 |
| I071 | `screenshots/structure_mep/load_cases_and_combinations_dialog.png` | Load Cases and Combinations dialog | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-83CC4548-F220-4C5B-A50B-82D42CE03460 | 200 |
| I072 | `screenshots/structure_mep/mep_fabrication_parts_model.png` | MEP fabrication parts model | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-A1B4B240-6EE4-4CF8-A9EE-AC989CC1FCD3 | 200 |
| I073 | `screenshots/structure_mep/panel_schedule_branch_two_column.png` | Branch panel schedule, circuits in two columns | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-6A8EC243-9414-4530-96B9-A413594A8B54 | 200 |
| I074 | `screenshots/structure_mep/panel_schedule_modify_ribbon.png` | Modify Panel Schedule ribbon | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-3F5CF335-1F1F-46D3-A781-3FE0B6209E57 | 200 |
| I075 | `screenshots/structure_mep/path_of_travel_with_schedule.png` | Path of travel with schedule | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-6E877A24-B52E-4039-8F44-1FBABB156189 | 200 |
| I076 | `screenshots/structure_mep/precast_shop_drawing.png` | Automatically generated precast shop drawing | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-90168BF1-32A4-40DE-819F-8D91999BBE14 | 200 |
| I077 | `screenshots/structure_mep/precast_wall_auto_reinforcement.png` | Precast wall assembly automatically reinforced | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-C38F239F-F919-499F-9F13-269C95E460BE | 200 |
| I078 | `screenshots/structure_mep/rebar_3d_path_shape_distribution_2027.png` | 3D path shape distribution along a double-curved element | https://help.autodesk.com/view/RVT/2027/ENU/?guid=GUID-C79E0E2E-6A74-4544-A134-A9167AA62310 | 200 |
| I079 | `screenshots/structure_mep/rebar_couplers.png` | Rebar couplers along a bar set | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-48C77E15-725F-413F-8001-6A1EBD03A1D7 | 200 |
| I080 | `screenshots/structure_mep/rebar_morphed_longitudinal_distribution_2027.png` | Morphed longitudinal bars following stirrups | https://help.autodesk.com/view/RVT/2027/ENU/?guid=GUID-E0821FE8-B1F1-4C4C-9350-C56D0DA6DD75 | 200 |
| I081 | `screenshots/structure_mep/rebar_partitions_by_level.png` | Rebar coloured by partition across three levels | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-9D989FA4-FE1F-4E85-B99D-BD33E005B420 | 200 |
| I082 | `screenshots/structure_mep/steel_custom_connections.png` | Custom steel connections examples | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-885D38DF-0FAE-4D35-B5DA-2FE75F3B420B | 200 |
| I083 | `screenshots/structure_mep/steel_fabrication_elements_plate_anchors.png` | Steel plate with anchors (fabrication elements) | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-0CB9D92E-F41A-46AB-A7D6-955F4B831425 | 200 |
| I084 | `screenshots/structure_mep/structural_connection_symbols.png` | Symbolic structural connection representation on a steel beam–column–brace join | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-14D5F295-680A-440B-848C-847363BA0D06 | 200 |
| I085 | `screenshots/structure_mep/structural_results_analysis_insights_2027.png` | Analysis Insights (2027) | https://help.autodesk.com/view/RVT/2027/ENU/?guid=GUID-515DEE6B-45D1-4A7E-867E-7330AE877B51 | 200 |
| I086 | `screenshots/structure_mep/structural_results_explorer_presets_2027.png` | Results Explorer presets (2027) | https://help.autodesk.com/view/RVT/2027/ENU/?guid=GUID-FC084260-A0E9-4F62-A136-6CDB3E514BE7 | 200 |
| I087 | `screenshots/structure_mep/system_browser_analytical_systems.png` | System Browser showing analytical systems | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-416C1FF7-1B42-4C17-ADDE-DC9D9954E49A | 200 |
| I088 | `screenshots/structure_mep/system_zones_plan.png` | System-zones plan | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-C4115E01-4FD5-406E-B738-5AB927235240 | 200 |
| I089 | `screenshots/structure_mep/systems_analysis_loads_report.png` | Systems Analysis loads report | https://help.autodesk.com/view/RVT/2026/ENU/?guid=GUID-EFFCBAEA-88B8-412C-B1DB-D0DFCBE15810 | 200 |
| I090 | `screenshots/documentation/3d_view_section_box.png` | 3D section box | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-BC219BE7-2917-47A8-9596-71C6C01659CF.htm | 200 |
| I091 | `screenshots/documentation/callout_views_detail.png` | Callout views | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-B05CE566-A607-42DC-99E4-7FAF442B31BA.htm | 200 |
| I092 | `screenshots/documentation/editing_request_received.png` | Editing request received | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-6A29E0EC-4720-498F-9C7F-F1BD97DDBABB.htm | 200 |
| I093 | `screenshots/documentation/keynote_legend_detail.png` | Keynote legend | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-4ED30197-CE07-477A-A093-6C397DE446CC.htm | 200 |
| I094 | `screenshots/documentation/manage_links_new_dialog_type_filter.png` | New Manage Links dialog | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-5E8B0871-E4F0-49A0-9B8F-E36C495E04C3.htm | 200 |
| I095 | `screenshots/documentation/pdf_export_dialog.png` | PDF Export dialog | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-BECFA1C6-5A10-4A40-92CB-0DCBE07D434C.htm | 200 |
| I096 | `screenshots/documentation/positioning_view_ribbon_saved_positions_2026.png` | Positioning & View ribbon panel | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-D8D3BAF0-587E-4D08-885C-FFC3504F056D.htm | 200 |
| I097 | `screenshots/documentation/rendered_image_raytracer.png` | Rendered image | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-AC50D6C6-56E2-40BF-96BF-EC1E31E0020B.htm | 200 |
| I098 | `screenshots/documentation/schedule_properties_filter_tab.png` | Schedule Properties — Filter tab | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/files/GUID-D42C5877-6384-46D4-BEE3-195F3DCDECF1.htm | 200 |
| I099 | `screenshots/documentation/schedule_room_finish_on_sheet.png` | Schedule on sheet | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/files/GUID-D42C5877-6384-46D4-BEE3-195F3DCDECF1.htm | 200 |
| I100 | `screenshots/documentation/schedule_view_room_finish_grouped_headers.png` | Schedule view with grouped headers | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/files/GUID-D42C5877-6384-46D4-BEE3-195F3DCDECF1.htm | 200 |
| I101 | `screenshots/documentation/section_view_building.png` | Section view | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-C8EDE015-4AE9-4A49-AB1B-AAEE04845B17.htm | 200 |
| I102 | `screenshots/documentation/sheet_collections_browser_print_set.png` | Sheet collections | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-3D4D534D-DDC8-4BBC-BC36-300566FA0766.htm | 200 |
| I103 | `screenshots/documentation/sheet_with_elevation_viewports.png` | Sheet with viewports | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-6D569C3B-B473-4AA9-BB2F-B93088D7D9A9.htm | 200 |
| I104 | `screenshots/documentation/synchronize_with_central_dialog.png` | Synchronize with Central dialog | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-7AF97381-C703-4ACE-8153-E328605CB018.htm | 200 |
| I105 | `screenshots/documentation/title_block_sheet_information.png` | Title block sheet info | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-C3BE8E62-7D0B-4B5C-B8DD-7D4CDDA960CE.htm | 200 |
| I106 | `screenshots/documentation/view_filter_rules_nested_sets.png` | Filter rules | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-745313E6-93F6-4FA2-A7D0-7D45242F5520.htm | 200 |
| I107 | `screenshots/documentation/view_range_cut_plane_diagram.png` | Cut plane effect | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-1245116F-DF47-4787-8C86-0607822CC702.htm | 200 |
| I108 | `screenshots/documentation/view_range_dialog.png` | View Range dialog | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-58711292-AB78-4C8F-BAA1-0855DDB518BF.htm | 200 |
| I109 | `screenshots/documentation/view_range_section_diagram.png` | View Range applied in section | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-58711292-AB78-4C8F-BAA1-0855DDB518BF.htm | 200 |
| I110 | `screenshots/documentation/view_range_view_depth_diagram.png` | View depth diagram | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-1245116F-DF47-4787-8C86-0607822CC702.htm | 200 |
| I111 | `screenshots/documentation/viewport_types_view_titles_sheet.png` | Viewport types / view titles | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-685D80C6-279C-4C47-9826-1483DDD0312E.htm | 200 |
| I112 | `screenshots/documentation/visibility_graphics_dialog_model_categories.png` | V/G dialog Model Categories | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-DocumentPresent/files/GUID-997DA39A-EC39-41BF-ACAC-975087E986E7.htm | 200 |
| I113 | `screenshots/documentation/worksets_dialog.png` | Worksets dialog | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-61C821EE-970C-46CC-B3BF-D03BE88E4288.htm | 200 |
| I114 | `screenshots/documentation/worksharing_central_local_diagram.png` | Worksharing diagram | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-0FC44807-DF06-4516-905A-4100281AC486.htm | 200 |
| I115 | `screenshots/documentation/worksharing_display_owners.png` | Worksharing display — owners | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-014966A2-2524-40F5-B033-B3E891BD2A59.htm | 200 |
| I116 | `screenshots/documentation/worksharing_display_worksets.png` | Worksharing display — worksets | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Collaborate/files/GUID-67BDEB80-2EAA-4F66-8562-D7C42A0F3A09.htm | 200 |
| I117 | `screenshots/platform/adaptive_component_repetition_2026.png` | Adaptive component repetition | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-DF875F40-BB7D-4E02-BF8D-2A552EEAFE21.png | 200 |
| I118 | `screenshots/platform/autodesk_assistant_2027.png` | Autodesk Assistant in Revit 2027 | https://kajabi-storefronts-production.kajabi-cdn.com/kajabi-storefronts-production/file-uploads/blogs/2147511612/images/446dcd7-6f8c-7ff1-2815-0bd85ac62b_AI-Assistant-Issue.png | 200 |
| I119 | `screenshots/platform/conceptual_divided_surface_patterned_2026.png` | Divided and patterned surface | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-BDFC8FB1-7042-404A-A284-282540137044.png | 200 |
| I120 | `screenshots/platform/conceptual_uv_grid_2026.png` | UV grid on surfaces | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-ArchDesign/images/GUID-9C2CD26E-A166-411E-B8C7-90A0D2203AB1.png | 200 |
| I121 | `screenshots/platform/dynamo_documentation_browser_2026.png` | Dynamo Documentation Browser | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/images/GUID-C36E20B6-0E91-493D-91A1-BAA53703185F.png | 200 |
| I122 | `screenshots/platform/dynamo_interface_library_canvas.jpg` | Dynamo interface | https://www.cad-notes.com/wp-content/uploads/2019/02/dynamo-working-interface.jpg | 200 |
| I123 | `screenshots/platform/dynamo_player_inputs_2026.png` | Dynamo Player inputs | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-HaveYouTried/images/GUID-4C86274A-864A-46B6-BE73-861614CB75C0.png | 200 |
| I124 | `screenshots/platform/dynamo_revit_node_library.jpg` | Dynamo Revit node library | https://primer2.dynamobim.org/7_dynamo_for_revit/1-the-revit-connection | 200 |
| I125 | `screenshots/platform/edit_label_dialog_2026.png` | Nested shared annotation and detail families | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-BFE9DB84-B549-496A-B2A5-DFAE8F28E740.png | 200 |
| I126 | `screenshots/platform/edit_shared_parameters_dialog.png` | Edit Shared Parameters dialog | https://eadn-wc01-10977100.nxedge.io/wp-content/uploads/2021/12/edit-shared-parameters-1.png | 200 |
| I127 | `screenshots/platform/family_types_cabinet_variations_2026.png` | Family types of a cabinet family | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-AA6627CC-24AA-43AD-8B10-3C1D7C846237.png | 200 |
| I128 | `screenshots/platform/family_types_dialog_parameter_groups.png` | Family Types dialog | https://static.wikitide.net/revitwiki/7/7c/Family_Parameters_-_Common.png | 200 |
| I129 | `screenshots/platform/formula_driven_sunshade_family_2026.png` | Swept blend and other forms constrained to references | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-547344A9-43D7-4B4B-AE05-4B95D85292C5.png | 200 |
| I130 | `screenshots/platform/ifc_property_mapping_2027.png` | IFC property mapping in 2027 | https://kajabi-storefronts-production.kajabi-cdn.com/kajabi-storefronts-production/file-uploads/blogs/2147511612/images/a3ae7d8-5530-1c55-b81d-8d6cdb5261b2_ifc-mapping.png | 200 |
| I131 | `screenshots/platform/location_and_site_dialog_2026.png` | Location and Site dialog | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-9689D74C-4C7F-4801-A14A-87DDF6C2B0A4.png | 200 |
| I132 | `screenshots/platform/macro_manager_and_vscode_2026.png` | Macro Manager with VS Code | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-F6D4C173-6DBD-4F7E-80DD-69F8AB5320E4.png | 200 |
| I133 | `screenshots/platform/macro_vscode_editor_2026.png` | Macro module code in VS Code | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-E8CD1B40-782B-486E-B691-80CFD8D53DF1.png | 200 |
| I134 | `screenshots/platform/manage_tab_settings_panel.png` | Manage tab Settings panel | https://eadn-wc01-10977100.nxedge.io/wp-content/uploads/2021/12/project-parameters.png | 200 |
| I135 | `screenshots/platform/manage_tab_visual_programming_panel.jpg` | Manage tab Visual Programming and Macros panels | https://primer2.dynamobim.org/7_dynamo_for_revit/1-the-revit-connection | 200 |
| I136 | `screenshots/platform/material_browser_appearance_tab_2026.png` | Material Browser – Appearance tab | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-7B935C59-BF24-4618-B441-B9B1B7C19888.png | 200 |
| I137 | `screenshots/platform/material_browser_physical_asset_annotated_2026.png` | Material Browser – Physical asset annotated | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Customize/images/GUID-041F5DC9-1961-4BC0-813F-39C2979DBE4C.png | 200 |
| I138 | `screenshots/platform/material_carbon_information_2027.png` | Carbon information on a material | https://kajabi-storefronts-production.kajabi-cdn.com/kajabi-storefronts-production/file-uploads/blogs/2147511612/images/3675aa-7d7c-6c5b-e4d-cf084cb2414c_carbon-information.png | 200 |
| I139 | `screenshots/platform/parameter_properties_shared_project_parameter.png` | Parameter Properties for a shared project parameter | https://eadn-wc01-10977100.nxedge.io/wp-content/uploads/2021/12/parameter-properties-dialog.png | 200 |
| I140 | `screenshots/platform/project_units_dialog.png` | Project Units dialog | https://www.cad-notes.com/wp-content/uploads/2015/06/Revit_project_units.png | 200 |
| I141 | `screenshots/platform/reporting_parameters_angles_2026.png` | Reference lines driving angle parameters | https://help.autodesk.com/cloudhelp/2026/ENU/Revit-Model/images/GUID-6CC4C825-7833-4169-9B8E-D244DDD3202B.png | 200 |
| I142 | `screenshots/platform/wall_tool_ribbon_no_options_bar_2027.png` | Wall tool ribbon in Revit 2027 (no Options Bar) | https://kajabi-storefronts-production.kajabi-cdn.com/kajabi-storefronts-production/file-uploads/blogs/2147511612/images/4f3d031-0cd6-84e0-65fe-35b028d34a7c_wall-tool-r-2027.png | 200 |
| S0832 | STR | Heaton Manufacturing, "Steel Reinforcing Mesh Sizes" (web search summary: BS 4483 fabrics A393, A252, A193, A142 at 10, 8, 7, 6 mm both ways at 200 mm; B503 and B385 8 and 7 mm at 100 mm with cross wires at 200 mm; standard sheets 4.8 × 2.4 m) | https://heatonmanufacturing.co.uk/reinforcing-mesh/ | 200 (search) | PASS: search-result summary only; facts of the standard |
| S0833 | STR | Wikipedia, "Welded wire mesh" (web search summary: WRI style designations spacing × W/D wire area in hundredths of in²; 6x6 W1.4/W1.4 … W4.0/W4.0; sheets 5 × 10 ft and 8 × 20 ft) | https://en.wikipedia.org/wiki/Welded_wire_mesh | 200 (search) | PASS: public encyclopaedia; search-result summary only |
| S0834 | STR | Web search summary: BS 8666 shape code 99, for shapes not given a code (and shapes of 5 bends or more other than links), drawn as a fully dimensioned sketch | https://crsteel.net/wp-content/uploads/2024/08/ShapeCode-SteelCalculator-MeshTypesNew.pdf | 200 (search) | PASS: search-result summary only; a fact of the standard (the code number and its use) |
| S0835 | UI | Anthropic, Claude Code CLI reference (`--mcp-config`: load MCP servers from JSON files or strings; `--strict-mcp-config`: only those) | https://code.claude.com/docs/en/cli-reference | 200 | PASS: public product documentation; an interface fact (command-line flags) |
| S0836 | UI | Gemini CLI configuration reference (user `~/.gemini/settings.json`, project `.gemini/settings.json`, system settings file and its override `GEMINI_CLI_SYSTEM_SETTINGS_PATH`, `mcpServers`) | https://geminicli.com/docs/reference/configuration | 200 | PASS: public product documentation; interface facts |
| S0837 | UI | Qwen Code, MCP servers (`mcpServers` in `~/.qwen/settings.json` or project `.qwen/settings.json`; stdio servers with command, args, env, cwd) | https://qwenlm.github.io/qwen-code-docs/en/developers/tools/mcp-server/ | 200 | PASS: public product documentation; interface facts |
| S0838 | UI | OpenAI, Codex MCP (stdio servers in `config.toml` with command, args, env) | https://learn.chatgpt.com/docs/extend/mcp?surface=cli | 200 | PASS: public product documentation; interface facts |
| S0839 | UI | OpenAI, Codex advanced configuration (web search summary: `-c`/`--config key=value` overrides, dotted keys such as `mcp_servers.<name>.<key>`, values parsed as TOML) | https://developers.openai.com/codex/config-advanced | 200 (search) | PASS: search-result summary of public documentation; interface facts |
| S0840 | MEP | Web search summary: fixture-unit demand tables (Hunter's curve) as reproduced from plumbing code appendices; the summaries disagreed and came from code tables, not the public-domain report | https://www.contractormag.com/plumbing/plumbing-experts-tackle-revisions-hunter-s-curve | 200 (search) | NOT USED: the values are code-table reproductions (possibly copyrighted) and inconsistent; no fixture-unit curve is shipped until Hunter's BMS 65 report itself is transcribed and checked |
| S0841 | MEP | Web search summary of buildingSMART IFC documentation for Pset_FireSuppressionTerminalTypeSprinkler (ActivationTemperature IfcThermodynamicTemperatureMeasure, DischargeCoefficient IfcReal "coefficient of flow at the sprinkler", Response, Activation, BulbLiquidColor enumerated) | https://standards.buildingsmart.org/IFC/RELEASE/IFC4_3/HTML/lexical/Pset_FireSuppressionTerminalTypeSprinkler.htm | 200 (search); 403 on fetch | PASS: public schema documentation; interface facts (property names and types) |
| S0842 | MEP | buildingSMART IFC4.3.x-development repository: Pset_FireSuppressionTerminalTypeSprinkler and the PEnum_SprinklerType, PEnum_SprinklerActivation, PEnum_SprinklerResponse and PEnum_SprinklerBulbLiquidColor item lists | https://github.com/buildingSMART/IFC4.3.x-development/tree/master/docs/schemas/domain/IfcPlumbingFireProtectionDomain | 200 | PASS: public schema source; enumeration item names only, as interface facts |
