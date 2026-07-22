from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.api_error_response import ApiErrorResponse
from typing import cast



def _get_kwargs(
    event_id: str,

) -> dict[str, Any]:
    

    

    

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/events/{event_id}/raw".format(event_id=quote(str(event_id), safe=""),),
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | ApiErrorResponse | None:
    if response.status_code == 200:
        response_200 = response.json()
        return response_200

    if response.status_code == 400:
        response_400 = ApiErrorResponse.from_dict(response.json())



        return response_400

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | ApiErrorResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    event_id: str,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | ApiErrorResponse]:
    """ GET /v1/events/{event_id}/raw — return the stored event JSON, or 404.
    Readable in any mode (only the PUT is dev-mode gated) so the frontend can
    fetch snapshots without dev mode. Public read route.

    Args:
        event_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse]
     """


    kwargs = _get_kwargs(
        event_id=event_id,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    event_id: str,
    *,
    client: AuthenticatedClient | Client,

) -> Any | ApiErrorResponse | None:
    """ GET /v1/events/{event_id}/raw — return the stored event JSON, or 404.
    Readable in any mode (only the PUT is dev-mode gated) so the frontend can
    fetch snapshots without dev mode. Public read route.

    Args:
        event_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse
     """


    return sync_detailed(
        event_id=event_id,
client=client,

    ).parsed

async def asyncio_detailed(
    event_id: str,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | ApiErrorResponse]:
    """ GET /v1/events/{event_id}/raw — return the stored event JSON, or 404.
    Readable in any mode (only the PUT is dev-mode gated) so the frontend can
    fetch snapshots without dev mode. Public read route.

    Args:
        event_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse]
     """


    kwargs = _get_kwargs(
        event_id=event_id,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    event_id: str,
    *,
    client: AuthenticatedClient | Client,

) -> Any | ApiErrorResponse | None:
    """ GET /v1/events/{event_id}/raw — return the stored event JSON, or 404.
    Readable in any mode (only the PUT is dev-mode gated) so the frontend can
    fetch snapshots without dev mode. Public read route.

    Args:
        event_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse
     """


    return (await asyncio_detailed(
        event_id=event_id,
client=client,

    )).parsed
