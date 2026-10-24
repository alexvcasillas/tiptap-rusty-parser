import { RootProvider } from "fumadocs-ui/provider/next";
import type { Metadata } from "next";
import "./global.css";
import { Inter } from "next/font/google";

const inter = Inter({
  subsets: ["latin"],
});

export const metadata: Metadata = {
  metadataBase: new URL("https://tiptap-studio.alexvcasillas.dev"),
  title: {
    template: "%s | Tiptap Studio",
    default: "Tiptap Studio",
  },
  description:
    "Desktop inspector, editor, differ and schema validator for Tiptap / ProseMirror JSON, and the Rust parser behind it.",
  icons: { icon: "/logo.svg" },
};

export default function Layout({ children }: LayoutProps<"/">) {
  return (
    <html lang="en" className={inter.className} suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <RootProvider theme={{ defaultTheme: "dark" }}>{children}</RootProvider>
      </body>
    </html>
  );
}
