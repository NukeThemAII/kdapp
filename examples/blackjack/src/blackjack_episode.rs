use borsh::{BorshDeserialize, BorshSerialize};
use kdapp::{
    episode::{Episode, EpisodeError, PayloadMetadata},
    pki::PubKey,
};
use log::info;
use rand::{seq::SliceRandom, thread_rng};

// --- Core Blackjack Game Structures ---

#[derive(Clone, Copy, Debug, BorshSerialize, BorshDeserialize, PartialEq, Eq)]
pub enum Suit {
    Hearts, Diamonds, Clubs, Spades,
}

#[derive(Clone, Copy, Debug, BorshSerialize, BorshDeserialize, PartialEq, Eq)]
pub enum Rank {
    Two, Three, Four, Five, Six, Seven, Eight, Nine, Ten, Jack, Queen, King, Ace,
}

impl Rank {
    fn value(&self) -> u8 {
        match self {
            Rank::Two => 2,
            Rank::Three => 3,
            Rank::Four => 4,
            Rank::Five => 5,
            Rank::Six => 6,
            Rank::Seven => 7,
            Rank::Eight => 8,
            Rank::Nine => 9,
            Rank::Ten | Rank::Jack | Rank::Queen | Rank::King => 10,
            Rank::Ace => 11,
        }
    }
}

#[derive(Clone, Copy, Debug, BorshSerialize, BorshDeserialize, PartialEq, Eq)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit,
}

#[derive(Clone, Debug, Default, BorshSerialize, BorshDeserialize)]
pub struct Hand {
    cards: Vec<Card>,
}

impl Hand {
    fn add_card(&mut self, card: Card) {
        self.cards.push(card);
    }

    fn value(&self) -> u8 {
        let mut score = self.cards.iter().map(|c| c.rank.value()).sum::<u8>();
        let mut aces = self.cards.iter().filter(|c| c.rank == Rank::Ace).count();

        while score > 21 && aces > 0 {
            score -= 10;
            aces -= 1;
        }
        score
    }
}

// --- Episode-Specific Structures ---

#[derive(Debug, BorshDeserialize, BorshSerialize)]
pub enum BlackjackError {
    InvalidCommand,
    NotPlayersTurn,
    GameOver,
    Unauthorized,
}

impl std::fmt::Display for BlackjackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlackjackError::InvalidCommand => write!(f, "Invalid command for the current game state."),
            BlackjackError::NotPlayersTurn => write!(f, "It's not this player's turn."),
            BlackjackError::GameOver => write!(f, "The game is already over."),
            BlackjackError::Unauthorized => write!(f, "Unauthorized participant."),
        }
    }
}
impl std::error::Error for BlackjackError {}


#[derive(Clone, Copy, Debug, BorshSerialize, BorshDeserialize)]
pub enum BlackjackCommand {
    Deal,
    Hit,
    Stand,
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub enum BlackjackGameStatus {
    Pending, // Waiting for Deal
    PlayerTurn,
    DealerTurn,
    Bust(PubKey),
    Winner(PubKey),
    Push, // Draw
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct BlackjackState {
    pub player_hand: Hand,
    pub dealer_hand: Hand, // In a real game, one card would be hidden
    pub status: BlackjackGameStatus,
}

impl BlackjackState {
    pub fn print(&self) {
        println!("--- Blackjack ---");
        println!("Player Hand ({}): {:?}", self.player_hand.value(), self.player_hand.cards);
        println!("Dealer Hand ({}): {:?}", self.dealer_hand.value(), self.dealer_hand.cards);
        match &self.status {
            BlackjackGameStatus::Pending => println!("Status: Ready to Deal"),
            BlackjackGameStatus::PlayerTurn => println!("Status: Player's Turn"),
            BlackjackGameStatus::DealerTurn => println!("Status: Dealer's Turn"),
            BlackjackGameStatus::Bust(pk) => println!("Status: Bust! {:?} loses.", pk),
            BlackjackGameStatus::Winner(pk) => println!("Status: Winner! {:?} wins.", pk),
            BlackjackGameStatus::Push => println!("Status: Push (Draw)"),
        }
        println!("-----------------");
    }
}


#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
pub enum BlackjackRollback {
    Deal,
    Hit,
    Stand,
}

#[derive(Clone, Debug)]
pub struct BlackjackEpisode {
    pub players: Vec<PubKey>, // [0] is player, [1] is dealer
    deck: Vec<Card>,
    player_hand: Hand,
    dealer_hand: Hand,
    status: BlackjackGameStatus,
    timestamp: u64,
}

impl Episode for BlackjackEpisode {
    type Command = BlackjackCommand;
    type CommandRollback = BlackjackRollback;
    type CommandError = BlackjackError;

    fn initialize(participants: Vec<PubKey>, metadata: &PayloadMetadata) -> Self {
        info!("[Blackjack] initialize: {:?}", participants);
        Self {
            players: participants,
            deck: Self::new_deck(),
            player_hand: Hand::default(),
            dealer_hand: Hand::default(),
            status: BlackjackGameStatus::Pending,
            timestamp: metadata.accepting_time,
        }
    }

    fn execute(
        &mut self,
        cmd: &Self::Command,
        authorization: Option<PubKey>,
        metadata: &PayloadMetadata,
    ) -> Result<Self::CommandRollback, EpisodeError<Self::CommandError>> {
        let player = authorization.ok_or(EpisodeError::Unauthorized)?;

        match cmd {
            BlackjackCommand::Deal => {
                if !matches!(self.status, BlackjackGameStatus::Pending) {
                    return Err(EpisodeError::InvalidCommand(BlackjackError::InvalidCommand));
                }
                self.deck = Self::new_deck();
                self.deck.shuffle(&mut thread_rng());
                self.player_hand = Hand::default();
                self.dealer_hand = Hand::default();

                self.player_hand.add_card(self.deck.pop().unwrap());
                self.dealer_hand.add_card(self.deck.pop().unwrap());
                self.player_hand.add_card(self.deck.pop().unwrap());
                self.dealer_hand.add_card(self.deck.pop().unwrap());

                self.status = BlackjackGameStatus::PlayerTurn;
                Ok(BlackjackRollback::Deal)
            }
            BlackjackCommand::Hit => {
                if !matches!(self.status, BlackjackGameStatus::PlayerTurn) || player != self.players[0] {
                    return Err(EpisodeError::InvalidCommand(BlackjackError::NotPlayersTurn));
                }
                self.player_hand.add_card(self.deck.pop().unwrap());
                if self.player_hand.value() > 21 {
                    self.status = BlackjackGameStatus::Bust(self.players[0]);
                }
                Ok(BlackjackRollback::Hit)
            }
            BlackjackCommand::Stand => {
                 if !matches!(self.status, BlackjackGameStatus::PlayerTurn) || player != self.players[0] {
                    return Err(EpisodeError::InvalidCommand(BlackjackError::NotPlayersTurn));
                }
                self.status = BlackjackGameStatus::DealerTurn;
                self.play_dealer_turn();
                Ok(BlackjackRollback::Stand)
            }
        }
    }

    fn rollback(&mut self, _rollback: Self::CommandRollback) -> bool {
        // For this simple version, we won't implement a full state rollback.
        // A real implementation would need to restore the deck and hands precisely.
        true
    }
}

impl BlackjackEpisode {
    fn new_deck() -> Vec<Card> {
        let suits = [Suit::Hearts, Suit::Diamonds, Suit::Clubs, Suit::Spades];
        let ranks = [
            Rank::Two, Rank::Three, Rank::Four, Rank::Five, Rank::Six, Rank::Seven,
            Rank::Eight, Rank::Nine, Rank::Ten, Rank::Jack, Rank::Queen, Rank::King, Rank::Ace,
        ];
        suits.iter().flat_map(|&suit| {
            ranks.iter().map(move |&rank| Card { suit, rank })
        }).collect()
    }

    fn play_dealer_turn(&mut self) {
        while self.dealer_hand.value() < 17 {
            self.dealer_hand.add_card(self.deck.pop().unwrap());
        }

        let player_score = self.player_hand.value();
        let dealer_score = self.dealer_hand.value();

        if dealer_score > 21 {
            self.status = BlackjackGameStatus::Winner(self.players[0]); // Player wins
        } else if dealer_score > player_score {
            self.status = BlackjackGameStatus::Winner(self.players[1]); // Dealer wins
        } else if dealer_score < player_score {
            self.status = BlackjackGameStatus::Winner(self.players[0]); // Player wins
        } else {
            self.status = BlackjackGameStatus::Push;
        }
    }

    pub fn poll(&self) -> BlackjackState {
        BlackjackState {
            player_hand: self.player_hand.clone(),
            dealer_hand: self.dealer_hand.clone(),
            status: self.status.clone(),
        }
    }
}