# Blackjack on Kaspa

This is an example of a decentralized Blackjack game built on the `kdapp` framework. It demonstrates how to create a simple, two-player (player vs. dealer) card game where game state is managed through Kaspa transactions.

## How it Works

The game logic is contained within a `BlackjackEpisode`. Players interact with the game by sending signed commands (`Deal`, `Hit`, `Stand`) as Kaspa transactions. The `kdapp` engine processes these transactions, updates the game state, and notifies the players.

The dealer's logic is automated within the episode. The deck is shuffled at the beginning of each round.

## How to Run

To play, you need to run two instances of the application: one as the "dealer" (the first player to initialize the game) and one as the "player".

**Prerequisites:**
*   You need a Kaspa wallet with some testnet funds to pay for transaction fees.

**Terminal 1 (Dealer):**

1.  Generate a game keypair (if you don't have one):
    ```bash
    cargo run --example blackjack
    ```
    This will output a `Player private key` and `Player public key`. Save these.

2.  Start the dealer instance. This will initialize the game episode. You need your Kaspa private key and the game keypair you just generated.
    ```bash
    cargo run --example blackjack -- \
      --kaspa-private-key <YOUR_KASPA_PRIVATE_KEY> \
      --game-private-key <YOUR_GAME_PRIVATE_KEY>
    ```
    The application will wait for an opponent.

**Terminal 2 (Player):**

1.  Generate another game keypair for the second player.
    ```bash
    cargo run --example blackjack
    ```
    Save this new keypair.

2.  Start the player instance. You will need your Kaspa private key, the new game private key, and the **public key** of the dealer (from Terminal 1).
    ```bash
    cargo run --example blackjack -- \
      --kaspa-private-key <YOUR_KASPA_PRIVATE_KEY> \
      --game-private-key <PLAYER_2_GAME_PRIVATE_KEY> \
      --game-opponent-key <DEALER_PUBLIC_KEY>
    ```

The game will now be initialized on the Kaspa network.

## How to Play

Once the game is running, you will be prompted for commands in the player's terminal (Terminal 2).

1.  **`deal`**: Type `deal` and press Enter to start a new round. You and the dealer will be dealt two cards each.
2.  **`hit`**: If you want another card, type `hit` and press Enter.
3.  **`stand`**: If you are satisfied with your hand, type `stand` and press Enter. The dealer will then play its turn according to standard Blackjack rules (hitting until 17 or more).

The game state, including hands and scores, will be printed in both terminals after each action.

After a round is over, you can type `deal` to start a new one. Type `exit` to quit.
