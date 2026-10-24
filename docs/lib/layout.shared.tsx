import type { BaseLayoutProps } from "fumadocs-ui/layouts/shared";
import { Logo } from "@/components/logo";
import { appName, gitConfig, releasesUrl } from "./shared";

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <>
          <Logo className="size-5" />
          <span className="font-semibold">{appName}</span>
        </>
      ),
    },
    links: [
      { text: "Docs", url: "/docs/studio", active: "nested-url" },
      { text: "Download", url: releasesUrl, external: true },
    ],
    githubUrl: `https://github.com/${gitConfig.user}/${gitConfig.repo}`,
  };
}
