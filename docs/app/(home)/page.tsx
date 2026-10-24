import Image from "next/image";
import Link from "next/link";
import { Logo } from "@/components/logo";
import { releasesUrl } from "@/lib/shared";

const features = [
  {
    title: "Inspect",
    body: "Tree, rendered preview, JSON source and an inspector, all on the same selection. Unknown node types render and stay editable.",
    href: "/docs/studio/inspect",
  },
  {
    title: "Edit structurally",
    body: "Insert, wrap, lift, split, join and move nodes, edit attributes, marks and raw JSON. Every edit is one undo step.",
    href: "/docs/studio/editing",
  },
  {
    title: "Diff and merge",
    body: "Compare two documents, take or dismiss each change, apply whole subtrees, and save the merged result.",
    href: "/docs/studio/diff",
  },
  {
    title: "Validate",
    body: "Check documents against a schema or the bundled StarterKit one, and step through every problem with F8.",
    href: "/docs/studio/schema",
  },
  {
    title: "Query",
    body: "Find nodes with a small query language: type == heading && attrs.level > 1.",
    href: "/docs/studio/find",
  },
  {
    title: "Document Server",
    body: "Browse documents and versions on a Tiptap Document Server, read-only, with tokens kept in your keychain.",
    href: "/docs/studio/document-server",
  },
];

const packages = [
  {
    name: "tiptap-rusty-parser",
    body: "The Rust crate behind the app: parse, query, mutate, diff and validate JSONContent.",
    href: "/docs/parser",
  },
  {
    name: "tiptap-rusty-markdown",
    body: "Markdown to JSONContent and back, CommonMark and GFM.",
    href: "/docs/markdown",
  },
  {
    name: "@tiptap-studio/rusty-parser",
    body: "The same engine for JavaScript, compiled to WebAssembly.",
    href: "/docs/javascript",
  },
];

export default function HomePage() {
  return (
    <main className="mx-auto flex w-full max-w-5xl flex-1 flex-col gap-20 px-6 py-20">
      <section className="flex flex-col items-center gap-6 text-center">
        <Logo className="size-14 text-fd-foreground" />
        <h1 className="text-4xl font-bold tracking-tight sm:text-5xl">
          Tiptap Studio
        </h1>
        <p className="max-w-2xl text-lg text-fd-muted-foreground">
          A desktop app to inspect, edit, diff and validate Tiptap and
          ProseMirror JSON documents. For macOS, Windows and Linux, built on a
          fast Rust parser.
        </p>
        <div className="flex flex-wrap justify-center gap-3">
          <a
            href={releasesUrl}
            className="rounded-lg bg-fd-primary px-5 py-2.5 font-medium text-fd-primary-foreground"
          >
            Download
          </a>
          <Link
            href="/docs/studio"
            className="rounded-lg border border-fd-border px-5 py-2.5 font-medium"
          >
            Read the docs
          </Link>
        </div>
      </section>

      <Image
        src="/screenshots/inspect.png"
        alt="Tiptap Studio inspecting a document: node tree, rendered preview and the inspector"
        width={2880}
        height={2168}
        priority
        className="w-full rounded-xl border border-fd-border shadow-2xl"
      />

      <section className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {features.map((f) => (
          <Link
            key={f.title}
            href={f.href}
            className="rounded-xl border border-fd-border bg-fd-card p-5 transition-colors hover:border-fd-primary"
          >
            <h2 className="mb-2 font-semibold">{f.title}</h2>
            <p className="text-sm text-fd-muted-foreground">{f.body}</p>
          </Link>
        ))}
      </section>

      <section className="flex flex-col gap-4">
        <h2 className="text-2xl font-semibold">Use the engine directly</h2>
        <div className="grid gap-4 sm:grid-cols-3">
          {packages.map((p) => (
            <Link
              key={p.name}
              href={p.href}
              className="rounded-xl border border-fd-border p-5 transition-colors hover:border-fd-primary"
            >
              <h3 className="mb-2 font-mono text-sm font-semibold">{p.name}</h3>
              <p className="text-sm text-fd-muted-foreground">{p.body}</p>
            </Link>
          ))}
        </div>
      </section>
    </main>
  );
}
