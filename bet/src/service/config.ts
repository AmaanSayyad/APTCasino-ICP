import { createClient, http } from "viem"
import { sepolia } from "viem/chains"
import { createConfig } from "wagmi"
import { injected, walletConnect } from "wagmi/connectors"

export const projectId = process.env.NEXT_PUBLIC_WC_PROJECT_ID!

export const config = createConfig({
  chains: [sepolia],
  connectors: [
    walletConnect({
      projectId,
      metadata: {
        name: "APt-Casino",
        description: "Apt-Casino, the decentralized casino for the blockchain",
        url: "https://github.com/",
        icons: ["https://avatars.githubusercontent.com/u/69464744?s=48&v=4"]
      }
    }),
    injected()
  ],
  client({ chain }) {
    return createClient({ chain, transport: http() })
  }
})
