//! Event handling types.

use crate as bevy_ecs;
use crate::system::{Local, Res, ResMut, SystemParam};
use bevy_utils::tracing::trace;
use std::{
    fmt::{self},
    hash::Hash,
    marker::PhantomData,
};

/// A type that can be stored in an [`Events<E>`] resource
/// You can conveniently access events using the [`EventReader`] and [`EventWriter`] system parameter.
///
/// Events must be thread-safe.
pub trait Event: Send + Sync + 'static {}
impl<T> Event for T where T: Send + Sync + 'static {}

/// An `EventId` uniquely identifies an event.
///
/// An `EventId` can among other things be used to trace the flow of an event from the point it was
/// sent to the point it was processed.
#[derive(Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct EventId<E: Event> {
    pub id: usize,
    _marker: PhantomData<E>,
}

impl<E: Event> Copy for EventId<E> {}
impl<E: Event> Clone for EventId<E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E: Event> fmt::Display for EventId<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        <Self as fmt::Debug>::fmt(self, f)
    }
}

impl<E: Event> fmt::Debug for EventId<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "event<{}>#{}",
            std::any::type_name::<E>().split("::").last().unwrap(),
            self.id,
        )
    }
}

#[derive(Debug)]
struct EventInstance<E: Event> {
    pub event_id: EventId<E>,
    pub event: E,
}

#[derive(Debug)]
enum State {
    A,
    B,
}

/// An event collection that represents the events that occurred within the last two
/// [`Events::update`] calls.
/// Events can be written to using an [`EventWriter`]
/// and are typically cheaply read using an [`EventReader`].
///
/// Each event can be consumed by multiple systems, in parallel,
/// with consumption tracked by the [`EventReader`] on a per-system basis.
///
/// If no [ordering](https://github.com/bevyengine/bevy/blob/main/examples/ecs/ecs_guide.rs)
/// is applied between writing and reading systems, there is a risk of a race condition.
/// This means that whether the events arrive before or after the next [`Events::update`] is unpredictable.
///
/// This collection is meant to be paired with a system that calls
/// [`Events::update`] exactly once per update/frame.
///
/// [`Events::update_system`] is a system that does this, typically intialized automatically using
/// [`add_event`](https://docs.rs/bevy/*/bevy/app/struct.App.html#method.add_event).
/// [`EventReader`]s are expected to read events from this collection at least once per loop/frame.
/// Events will persist across a single frame boundary and so ordering of event producers and
/// consumers is not critical (although poorly-planned ordering may cause accumulating lag).
/// If events are not handled by the end of the frame after they are updated, they will be
/// dropped silently.
///
/// # Example
/// ```
/// use bevy_ecs::event::Events;
///
/// struct MyEvent {
///     value: usize
/// }
///
/// // setup
/// let mut events = Events::<MyEvent>::default();
/// let mut reader = events.get_reader();
///
/// // run this once per update/frame
/// events.update();
///
/// // somewhere else: send an event
/// events.send(MyEvent { value: 1 });
///
/// // somewhere else: read the events
/// for event in reader.iter(&events) {
///     assert_eq!(event.value, 1)
/// }
///
/// // events are only processed once per reader
/// assert_eq!(reader.iter(&events).count(), 0);
/// ```
///
/// # Details
///
/// [`Events`] is implemented using a variation of a double buffer strategy.
/// Each call to [`update`](Events::update) swaps buffers and clears out the oldest one.
/// - [`EventReader`]s will read events from both buffers.
/// - [`EventReader`]s that read at least once per update will never drop events.
/// - [`EventReader`]s that read once within two updates might still receive some events
/// - [`EventReader`]s that read after two updates are guaranteed to drop all events that occurred
/// before those updates.
///
/// The buffers in [`Events`] will grow indefinitely if [`update`](Events::update) is never called.
///
/// An alternative call pattern would be to call [`update`](Events::update)
/// manually across frames to control when events are cleared.
/// This complicates consumption and risks ever-expanding memory usage if not cleaned up,
/// but can be done by adding your event as a resource instead of using
/// [`add_event`](https://docs.rs/bevy/*/bevy/app/struct.App.html#method.add_event).
///
/// [Example usage.](https://github.com/bevyengine/bevy/blob/latest/examples/ecs/event.rs)
/// [Example usage standalone.](https://github.com/bevyengine/bevy/blob/latest/bevy_ecs/examples/events.rs)
///
#[derive(Debug)]
pub struct Events<E: Event> {
    events_a: Vec<EventInstance<E>>,
    events_b: Vec<EventInstance<E>>,
    a_start_event_count: usize,
    b_start_event_count: usize,
    event_count: usize,
    state: State,
}

impl<E: Event> Default for Events<E> {
    fn default() -> Self {
        Events {
            a_start_event_count: 0,
            b_start_event_count: 0,
            event_count: 0,
            events_a: Vec::new(),
            events_b: Vec::new(),
            state: State::A,
        }
    }
}

fn map_instance_event_with_id<E: Event>(event_instance: &EventInstance<E>) -> (&E, EventId<E>) {
    (&event_instance.event, event_instance.event_id)
}

fn map_instance_event<E: Event>(event_instance: &EventInstance<E>) -> &E {
    &event_instance.event
}

/// Reads events of type `T` in order and tracks which events have already been read.
#[derive(SystemParam)]
pub struct EventReader<'w, 's, E: Event> {
    last_event_count: Local<'s, (usize, PhantomData<E>)>,
    events: Res<'w, Events<E>>,
}

/// Sends events of type `T`.
#[derive(SystemParam)]
pub struct EventWriter<'w, 's, E: Event> {
    events: ResMut<'w, Events<E>>,
    #[system_param(ignore)]
    marker: PhantomData<&'s usize>,
}

impl<'w, 's, E: Event> EventWriter<'w, 's, E> {
    /// Sends an `event`. [`EventReader`]s can then read the event.
    /// See [`Events`] for details.
    pub fn send(&mut self, event: E) {
        self.events.send(event);
    }

    pub fn send_batch(&mut self, events: impl Iterator<Item = E>) {
        self.events.extend(events);
    }

    /// Sends the default value of the event. Useful when the event is an empty struct.
    pub fn send_default(&mut self)
    where
        E: Default,
    {
        self.events.send_default();
    }
}

pub struct ManualEventReader<E: Event> {
    last_event_count: usize,
    _marker: PhantomData<E>,
}

impl<E: Event> Default for ManualEventReader<E> {
    fn default() -> Self {
        ManualEventReader {
            last_event_count: 0,
            _marker: Default::default(),
        }
    }
}

#[allow(clippy::len_without_is_empty)] // Check fails since the is_empty implementation has a signature other than `(&self) -> bool`
impl<E: Event> ManualEventReader<E> {
    /// See [`EventReader::iter`]
    pub fn iter<'a>(
        &'a mut self,
        events: &'a Events<E>,
    ) -> impl DoubleEndedIterator<Item = &'a E> + ExactSizeIterator<Item = &'a E> {
        internal_event_reader(&mut self.last_event_count, events).map(|(e, _)| e)
    }

    /// See [`EventReader::iter_with_id`]
    pub fn iter_with_id<'a>(
        &'a mut self,
        events: &'a Events<E>,
    ) -> impl DoubleEndedIterator<Item = (&'a E, EventId<E>)>
           + ExactSizeIterator<Item = (&'a E, EventId<E>)> {
        internal_event_reader(&mut self.last_event_count, events)
    }

    /// See [`EventReader::len`]
    pub fn len(&self, events: &Events<E>) -> usize {
        internal_event_reader(&mut self.last_event_count.clone(), events).len()
    }

    /// See [`EventReader::is_empty`]
    pub fn is_empty(&self, events: &Events<E>) -> bool {
        self.len(events) == 0
    }
}

/// Like [`iter_with_id`](EventReader::iter_with_id) except not emitting any traces for read
/// messages.
fn internal_event_reader<'a, E: Event>(
    last_event_count: &'a mut usize,
    events: &'a Events<E>,
) -> impl DoubleEndedIterator<Item = (&'a E, EventId<E>)> + ExactSizeIterator<Item = (&'a E, EventId<E>)>
{
    // if the reader has seen some of the events in a buffer, find the proper index offset.
    // otherwise read all events in the buffer
    let a_index = if *last_event_count > events.a_start_event_count {
        *last_event_count - events.a_start_event_count
    } else {
        0
    };
    let b_index = if *last_event_count > events.b_start_event_count {
        *last_event_count - events.b_start_event_count
    } else {
        0
    };
    let a = events.events_a.get(a_index..).unwrap_or_default();
    let b = events.events_b.get(b_index..).unwrap_or_default();
    let unread_count = a.len() + b.len();
    *last_event_count = events.event_count - unread_count;
    let iterator = match events.state {
        State::A => b.iter().chain(a.iter()),
        State::B => a.iter().chain(b.iter()),
    };
    iterator
        .map(map_instance_event_with_id)
        .with_exact_size(unread_count)
        .inspect(move |(_, id)| *last_event_count = (id.id + 1).max(*last_event_count))
}

trait IteratorExt {
    fn with_exact_size(self, len: usize) -> ExactSize<Self>
    where
        Self: Sized,
    {
        ExactSize::new(self, len)
    }
}
impl<I> IteratorExt for I where I: Iterator {}

#[must_use = "iterators are lazy and do nothing unless consumed"]
#[derive(Clone)]
struct ExactSize<I> {
    iter: I,
    len: usize,
}
impl<I> ExactSize<I> {
    fn new(iter: I, len: usize) -> Self {
        ExactSize { iter, len }
    }
}

impl<I: Iterator> Iterator for ExactSize<I> {
    type Item = I::Item;

    #[inline]
    fn next(&mut self) -> Option<I::Item> {
        self.iter.next().map(|e| {
            self.len -= 1;
            e
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<I: DoubleEndedIterator> DoubleEndedIterator for ExactSize<I> {
    #[inline]
    fn next_back(&mut self) -> Option<I::Item> {
        self.iter.next_back().map(|e| {
            self.len -= 1;
            e
        })
    }
}
impl<I: Iterator> ExactSizeIterator for ExactSize<I> {
    fn len(&self) -> usize {
        self.len
    }
}

impl<'w, 's, E: Event> EventReader<'w, 's, E> {
    /// Iterates over the events this [`EventReader`] has not seen yet. This updates the
    /// [`EventReader`]'s event counter, which means subsequent event reads will not include events
    /// that happened before now.
    pub fn iter(&mut self) -> impl DoubleEndedIterator<Item = &E> + ExactSizeIterator<Item = &E> {
        self.iter_with_id().map(|(event, _id)| event)
    }

    /// Like [`iter`](Self::iter), except also returning the [`EventId`] of the events.
    pub fn iter_with_id(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = (&E, EventId<E>)> + ExactSizeIterator<Item = (&E, EventId<E>)>
    {
        internal_event_reader(&mut self.last_event_count.0, &self.events).map(|(event, id)| {
            trace!("EventReader::iter() -> {}", id);
            (event, id)
        })
    }

    /// Determines the number of events available to be read from this [`EventReader`] without consuming any.
    pub fn len(&self) -> usize {
        internal_event_reader(&mut self.last_event_count.0.clone(), &self.events).len()
    }

    /// Determines if are any events available to be read without consuming any.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<E: Event> Events<E> {
    /// "Sends" an `event` by writing it to the current event buffer. [`EventReader`]s can then read
    /// the event.
    pub fn send(&mut self, event: E) {
        let event_id = EventId {
            id: self.event_count,
            _marker: PhantomData,
        };
        trace!("Events::send() -> id: {}", event_id);

        let event_instance = EventInstance { event_id, event };

        match self.state {
            State::A => self.events_a.push(event_instance),
            State::B => self.events_b.push(event_instance),
        }

        self.event_count += 1;
    }

    /// Sends the default value of the event. Useful when the event is an empty struct.
    pub fn send_default(&mut self)
    where
        E: Default,
    {
        self.send(Default::default());
    }

    /// Gets a new [`ManualEventReader`]. This will include all events already in the event buffers.
    pub fn get_reader(&self) -> ManualEventReader<E> {
        ManualEventReader {
            last_event_count: 0,
            _marker: PhantomData,
        }
    }

    /// Gets a new [`ManualEventReader`]. This will ignore all events already in the event buffers.
    /// It will read all future events.
    pub fn get_reader_current(&self) -> ManualEventReader<E> {
        ManualEventReader {
            last_event_count: self.event_count,
            _marker: PhantomData,
        }
    }

    /// Swaps the event buffers and clears the oldest event buffer. In general, this should be
    /// called once per frame/update.
    pub fn update(&mut self) {
        match self.state {
            State::A => {
                self.events_b.clear();
                self.state = State::B;
                self.b_start_event_count = self.event_count;
            }
            State::B => {
                self.events_a.clear();
                self.state = State::A;
                self.a_start_event_count = self.event_count;
            }
        }
    }

    /// A system that calls [`Events::update`] once per frame.
    pub fn update_system(mut events: ResMut<Self>) {
        events.update();
    }

    #[inline]
    fn reset_start_event_count(&mut self) {
        self.a_start_event_count = self.event_count;
        self.b_start_event_count = self.event_count;
    }

    /// Removes all events.
    #[inline]
    pub fn clear(&mut self) {
        self.reset_start_event_count();
        self.events_a.clear();
        self.events_b.clear();
    }

    /// Returns true if there are no events in this collection.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.events_a.is_empty() && self.events_b.is_empty()
    }

    /// Creates a draining iterator that removes all events.
    pub fn drain(&mut self) -> impl Iterator<Item = E> + '_ {
        self.reset_start_event_count();

        let map = |i: EventInstance<E>| i.event;
        match self.state {
            State::A => self
                .events_b
                .drain(..)
                .map(map)
                .chain(self.events_a.drain(..).map(map)),
            State::B => self
                .events_a
                .drain(..)
                .map(map)
                .chain(self.events_b.drain(..).map(map)),
        }
    }

    /// Iterates over events that happened since the last "update" call.
    /// WARNING: You probably don't want to use this call. In most cases you should use an
    /// [`EventReader`]. You should only use this if you know you only need to consume events
    /// between the last `update()` call and your call to `iter_current_update_events`.
    /// If events happen outside that window, they will not be handled. For example, any events that
    /// happen after this call and before the next `update()` call will be dropped.
    pub fn iter_current_update_events(
        &self,
    ) -> impl DoubleEndedIterator<Item = &E> + ExactSizeIterator<Item = &E> {
        match self.state {
            State::A => self.events_a.iter().map(map_instance_event),
            State::B => self.events_b.iter().map(map_instance_event),
        }
    }
}

impl<E: Event> std::iter::Extend<E> for Events<E> {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = E>,
    {
        let mut event_count = self.event_count;
        let events = iter.into_iter().map(|event| {
            let event_id = EventId {
                id: event_count,
                _marker: PhantomData,
            };
            event_count += 1;
            EventInstance { event_id, event }
        });

        match self.state {
            State::A => self.events_a.extend(events),
            State::B => self.events_b.extend(events),
        }

        trace!(
            "Events::extend() -> ids: ({}..{})",
            self.event_count,
            event_count
        );
        self.event_count = event_count;
    }
}

#[cfg(test)]
mod tests {
    use crate::{prelude::World, system::SystemState};

    use super::*;

    #[derive(Copy, Clone, PartialEq, Eq, Debug)]
    struct TestEvent {
        i: usize,
    }

    #[test]
    fn test_events() {
        let mut events = Events::<TestEvent>::default();
        let event_0 = TestEvent { i: 0 };
        let event_1 = TestEvent { i: 1 };
        let event_2 = TestEvent { i: 2 };

        // this reader will miss event_0 and event_1 because it wont read them over the course of
        // two updates
        let mut reader_missed = events.get_reader();

        let mut reader_a = events.get_reader();

        events.send(event_0);

        assert_eq!(
            get_events(&events, &mut reader_a),
            vec![event_0],
            "reader_a created before event receives event"
        );
        assert_eq!(
            get_events(&events, &mut reader_a),
            vec![],
            "second iteration of reader_a created before event results in zero events"
        );

        let mut reader_b = events.get_reader();

        assert_eq!(
            get_events(&events, &mut reader_b),
            vec![event_0],
            "reader_b created after event receives event"
        );
        assert_eq!(
            get_events(&events, &mut reader_b),
            vec![],
            "second iteration of reader_b created after event results in zero events"
        );

        events.send(event_1);

        let mut reader_c = events.get_reader();

        assert_eq!(
            get_events(&events, &mut reader_c),
            vec![event_0, event_1],
            "reader_c created after two events receives both events"
        );
        assert_eq!(
            get_events(&events, &mut reader_c),
            vec![],
            "second iteration of reader_c created after two event results in zero events"
        );

        assert_eq!(
            get_events(&events, &mut reader_a),
            vec![event_1],
            "reader_a receives next unread event"
        );

        events.update();

        let mut reader_d = events.get_reader();

        events.send(event_2);

        assert_eq!(
            get_events(&events, &mut reader_a),
            vec![event_2],
            "reader_a receives event created after update"
        );
        assert_eq!(
            get_events(&events, &mut reader_b),
            vec![event_1, event_2],
            "reader_b receives events created before and after update"
        );
        assert_eq!(
            get_events(&events, &mut reader_d),
            vec![event_0, event_1, event_2],
            "reader_d receives all events created before and after update"
        );

        events.update();

        assert_eq!(
            get_events(&events, &mut reader_missed),
            vec![event_2],
            "reader_missed missed events unread after two update() calls"
        );
    }

    fn get_events<E: Event + Clone>(
        events: &Events<E>,
        reader: &mut ManualEventReader<E>,
    ) -> Vec<E> {
        reader.iter(events).cloned().collect::<Vec<E>>()
    }

    #[derive(PartialEq, Eq, Debug)]
    struct E(usize);

    fn events_clear_and_read_impl(clear_func: impl FnOnce(&mut Events<E>)) {
        let mut events = Events::<E>::default();
        let mut reader = events.get_reader();

        assert!(reader.iter(&events).next().is_none());

        events.send(E(0));
        assert_eq!(*reader.iter(&events).next().unwrap(), E(0));
        assert_eq!(reader.iter(&events).next(), None);

        events.send(E(1));
        clear_func(&mut events);
        assert!(reader.iter(&events).next().is_none());

        events.send(E(2));
        events.update();
        events.send(E(3));

        assert!(reader.iter(&events).eq([E(2), E(3)].iter()));
    }

    #[test]
    fn test_events_clear_and_read() {
        events_clear_and_read_impl(|events| events.clear());
    }

    #[test]
    fn test_events_drain_and_read() {
        events_clear_and_read_impl(|events| {
            assert!(events.drain().eq(vec![E(0), E(1)].into_iter()));
        });
    }

    #[test]
    fn test_events_extend_impl() {
        let mut events = Events::<TestEvent>::default();
        let mut reader = events.get_reader();

        events.extend(vec![TestEvent { i: 0 }, TestEvent { i: 1 }]);
        assert!(reader
            .iter(&events)
            .eq([TestEvent { i: 0 }, TestEvent { i: 1 }].iter()));
    }

    #[test]
    fn test_events_empty() {
        let mut events = Events::<TestEvent>::default();
        assert!(events.is_empty());

        events.send(TestEvent { i: 0 });
        assert!(!events.is_empty());

        events.update();
        assert!(!events.is_empty());

        // events are only empty after the second call to update
        // due to double buffering.
        events.update();
        assert!(events.is_empty());
    }

    #[test]
    fn test_event_reader_len_empty() {
        let events = Events::<TestEvent>::default();
        assert_eq!(events.get_reader().len(&events), 0);
        assert!(events.get_reader().is_empty(&events));
    }

    #[test]
    fn test_event_reader_len_filled() {
        let mut events = Events::<TestEvent>::default();
        events.send(TestEvent { i: 0 });
        assert_eq!(events.get_reader().len(&events), 1);
        assert!(!events.get_reader().is_empty(&events));
    }

    #[test]
    fn test_event_iter_len_updated() {
        let mut events = Events::<TestEvent>::default();
        events.send(TestEvent { i: 0 });
        events.send(TestEvent { i: 1 });
        events.send(TestEvent { i: 2 });
        let mut reader = events.get_reader();
        let mut iter = reader.iter(&events);
        assert_eq!(iter.len(), 3);
        iter.next();
        assert_eq!(iter.len(), 2);
        iter.next_back();
        assert_eq!(iter.len(), 1);
    }

    #[test]
    fn test_event_reader_len_current() {
        let mut events = Events::<TestEvent>::default();
        events.send(TestEvent { i: 0 });
        let reader = events.get_reader_current();
        assert!(reader.is_empty(&events));
        events.send(TestEvent { i: 0 });
        assert_eq!(reader.len(&events), 1);
        assert!(!reader.is_empty(&events));
    }

    #[test]
    fn test_event_reader_len_update() {
        let mut events = Events::<TestEvent>::default();
        events.send(TestEvent { i: 0 });
        events.send(TestEvent { i: 0 });
        let reader = events.get_reader();
        assert_eq!(reader.len(&events), 2);
        events.update();
        events.send(TestEvent { i: 0 });
        assert_eq!(reader.len(&events), 3);
        events.update();
        assert_eq!(reader.len(&events), 1);
        events.update();
        assert!(reader.is_empty(&events));
    }

    #[derive(Clone, PartialEq, Debug, Default)]
    struct EmptyTestEvent;

    #[test]
    fn test_firing_empty_event() {
        let mut events = Events::<EmptyTestEvent>::default();
        events.send_default();

        let mut reader = events.get_reader();
        assert_eq!(
            get_events(&events, &mut reader),
            vec![EmptyTestEvent::default()]
        );
    }

    #[test]
    fn ensure_reader_readonly() {
        fn read_for<E: Event>() {
            let mut world = World::new();
            world.init_resource::<Events<E>>();
            let mut state = SystemState::<EventReader<E>>::new(&mut world);
            // This can only work if EventReader only reads the world
            let _reader = state.get(&world);
        }
        read_for::<EmptyTestEvent>();
    }
}
