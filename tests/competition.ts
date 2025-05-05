import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { GmsolCompetition } from "../target/types/gmsol_competition";
import BN from "bn.js";

describe("gmsol-competition", () => {
  const provider   = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program    = anchor.workspace.GmsolCompetition as Program<GmsolCompetition>;

  const competitionKeypair = anchor.web3.Keypair.generate();

  it("initializes competition", async () => {
    const now = Math.floor(Date.now() / 1000);
    await program.methods
      .initializeCompetition(new BN(now), new BN(now + 3600), program.programId)
      .accounts({
        competition: competitionKeypair.publicKey,
        authority:   provider.wallet.publicKey
      })
      .signers([competitionKeypair])
      .rpc();

    const account = await program.account.competition.fetch(competitionKeypair.publicKey);
    console.log("competition:", account);
  });
});