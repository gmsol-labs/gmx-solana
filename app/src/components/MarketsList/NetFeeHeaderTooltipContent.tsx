import { Trans } from "@lingui/macro";
import ExternalLink from "@/components/ExternalLink/ExternalLink";

import "./NetFeeHeaderTooltipContent.scss";

export function renderNetFeeHeaderTooltipContent() {
  return (
    <div className="NetFeeHeaderTooltipContent-netfee-header-tooltip">
      <Trans>
        Net fee combines funding and borrowing fees but excludes open, swap or impact fees.
        <br />
        <br />
        Funding fees help to balance longs and shorts and are exchanged between both sides.{" "}
        <ExternalLink newTab href="#">
          Read more
        </ExternalLink>
        .
        <br />
        <br />
        Borrowing fees help ensure available liquidity.{" "}
        <ExternalLink newTab href="#">
          Read more
        </ExternalLink>
        .
      </Trans>
    </div>
  );
}
